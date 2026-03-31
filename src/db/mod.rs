pub mod types;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Arc;
use tracing::error;

use types::{Project, Session, SessionStatus, SessionWithProject};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(db_path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        Self::migrate(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn migrate(conn: &Connection) -> rusqlite::Result<()> {
        let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS projects (
                    channel_id TEXT PRIMARY KEY,
                    project_path TEXT NOT NULL,
                    guild_id TEXT NOT NULL,
                    auto_approve INTEGER DEFAULT 0,
                    model TEXT DEFAULT NULL,
                    disabled_mcps TEXT DEFAULT NULL,
                    created_at TEXT DEFAULT (datetime('now'))
                );

                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    channel_id TEXT REFERENCES projects(channel_id) ON DELETE CASCADE,
                    session_id TEXT,
                    status TEXT DEFAULT 'offline'
                        CHECK(status IN ('online', 'offline', 'waiting', 'idle')),
                    last_activity TEXT,
                    created_at TEXT DEFAULT (datetime('now'))
                );

                CREATE INDEX IF NOT EXISTS idx_sessions_channel_id ON sessions(channel_id);
                CREATE INDEX IF NOT EXISTS idx_projects_guild_id ON projects(guild_id);
            ",
            )?;
        }

        // Future migrations go here:
        // if version < 2 { ... }

        conn.pragma_update(None, "user_version", 1)?;
        Ok(())
    }

    // --- Project queries ---

    pub fn register_project(
        &self,
        channel_id: &str,
        project_path: &str,
        guild_id: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO projects (channel_id, project_path, guild_id) VALUES (?1, ?2, ?3)
             ON CONFLICT(channel_id) DO UPDATE SET project_path = ?2, guild_id = ?3",
            params![channel_id, project_path, guild_id],
        )?;
        Ok(())
    }

    pub fn unregister_project(&self, channel_id: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM projects WHERE channel_id = ?1",
            params![channel_id],
        )?;
        Ok(())
    }

    pub fn get_project(&self, channel_id: &str) -> Option<Project> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT channel_id, project_path, guild_id, auto_approve, created_at FROM projects WHERE channel_id = ?1",
            params![channel_id],
            |row| {
                Ok(Project {
                    channel_id: row.get(0)?,
                    project_path: row.get(1)?,
                    guild_id: row.get(2)?,
                    auto_approve: row.get::<_, i32>(3)? != 0,
                    created_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| { error!("get_project({channel_id}): {e}"); e })
        .ok()
        .flatten()
    }

    pub fn get_all_projects(&self, guild_id: &str) -> Vec<Project> {
        let conn = self.conn.lock();
        let mut stmt = match conn
            .prepare("SELECT channel_id, project_path, guild_id, auto_approve, created_at FROM projects WHERE guild_id = ?1")
        {
            Ok(s) => s,
            Err(e) => { error!("get_all_projects({guild_id}): {e}"); return vec![]; }
        };
        match stmt.query_map(params![guild_id], |row| {
            Ok(Project {
                channel_id: row.get(0)?,
                project_path: row.get(1)?,
                guild_id: row.get(2)?,
                auto_approve: row.get::<_, i32>(3)? != 0,
                created_at: row.get(4)?,
            })
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                error!("get_all_projects({guild_id}) query: {e}");
                vec![]
            }
        }
    }

    pub fn set_auto_approve(&self, channel_id: &str, auto_approve: bool) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE projects SET auto_approve = ?1 WHERE channel_id = ?2",
            params![auto_approve as i32, channel_id],
        )?;
        Ok(())
    }

    // --- Session queries ---

    pub fn upsert_session(
        &self,
        db_id: &str,
        channel_id: &str,
        claude_session_id: Option<&str>,
        status: SessionStatus,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO sessions (id, channel_id, session_id, status, last_activity) VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            params![db_id, channel_id, claude_session_id, status.as_str()],
        )?;
        Ok(())
    }

    pub fn get_session(&self, channel_id: &str) -> Option<Session> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, channel_id, session_id, status, last_activity, created_at FROM sessions WHERE channel_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![channel_id],
            |row| {
                Ok(Session {
                    db_id: row.get(0)?,
                    channel_id: row.get(1)?,
                    claude_session_id: row.get(2)?,
                    status: SessionStatus::parse(&row.get::<_, String>(3)?),
                    last_activity: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|e| { error!("get_session({channel_id}): {e}"); e })
        .ok()
        .flatten()
    }

    pub fn clear_session(&self, channel_id: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM sessions WHERE channel_id = ?1",
            params![channel_id],
        )?;
        Ok(())
    }

    pub fn update_session_status(
        &self,
        channel_id: &str,
        status: SessionStatus,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET status = ?1, last_activity = datetime('now') WHERE channel_id = ?2",
            params![status.as_str(), channel_id],
        )?;
        Ok(())
    }

    /// Atomically read the old status and update to the new status within a single lock.
    pub fn swap_session_status(
        &self,
        channel_id: &str,
        new_status: SessionStatus,
    ) -> SessionStatus {
        let conn = self.conn.lock();
        let old: String = conn
            .query_row(
                "SELECT status FROM sessions WHERE channel_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![channel_id],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "offline".to_string());
        let _ = conn.execute(
            "UPDATE sessions SET status = ?1, last_activity = datetime('now') WHERE channel_id = ?2",
            params![new_status.as_str(), channel_id],
        );
        SessionStatus::parse(&old)
    }

    pub fn get_all_sessions(&self, guild_id: &str) -> Vec<SessionWithProject> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT s.id, s.channel_id, s.session_id, s.status, s.last_activity, s.created_at, p.project_path
             FROM sessions s JOIN projects p ON s.channel_id = p.channel_id
             WHERE p.guild_id = ?1",
        ) {
            Ok(s) => s,
            Err(e) => { error!("get_all_sessions({guild_id}): {e}"); return vec![]; }
        };
        match stmt.query_map(params![guild_id], |row| {
            Ok(SessionWithProject {
                session: Session {
                    db_id: row.get(0)?,
                    channel_id: row.get(1)?,
                    claude_session_id: row.get(2)?,
                    status: SessionStatus::parse(&row.get::<_, String>(3)?),
                    last_activity: row.get(4)?,
                    created_at: row.get(5)?,
                },
                project_path: row.get(6)?,
            })
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                error!("get_all_sessions({guild_id}) query: {e}");
                vec![]
            }
        }
    }

    // --- Model per channel ---

    pub fn set_model(&self, channel_id: &str, model: Option<&str>) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE projects SET model = ?1 WHERE channel_id = ?2",
            params![model, channel_id],
        )?;
        Ok(())
    }

    pub fn get_model(&self, channel_id: &str) -> Option<String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT model FROM projects WHERE channel_id = ?1",
            params![channel_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(|e| {
            error!("get_model({channel_id}): {e}");
            e
        })
        .ok()
        .flatten()
    }

    // --- Disabled MCPs per channel ---

    pub fn get_disabled_mcps(&self, channel_id: &str) -> Vec<String> {
        let conn = self.conn.lock();
        let raw: Option<String> = conn
            .query_row(
                "SELECT disabled_mcps FROM projects WHERE channel_id = ?1",
                params![channel_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                error!("get_disabled_mcps({channel_id}): {e}");
                e
            })
            .ok()
            .flatten();
        match raw {
            Some(s) => serde_json::from_str(&s).unwrap_or_default(),
            None => vec![],
        }
    }

    /// Returns (online, waiting, idle) counts across all sessions.
    pub fn get_session_status_counts(&self) -> (u32, u32, u32) {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare("SELECT status, COUNT(*) FROM sessions GROUP BY status") {
            Ok(s) => s,
            Err(e) => {
                error!("get_session_status_counts: {e}");
                return (0, 0, 0);
            }
        };
        let mut online = 0u32;
        let mut waiting = 0u32;
        let mut idle = 0u32;
        let rows = match stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        }) {
            Ok(rows) => rows,
            Err(e) => {
                error!("get_session_status_counts query: {e}");
                return (0, 0, 0);
            }
        };
        for row in rows.flatten() {
            match row.0.as_str() {
                "online" => online = row.1,
                "waiting" => waiting = row.1,
                "idle" => idle = row.1,
                _ => {}
            }
        }
        (online, waiting, idle)
    }

    pub fn set_disabled_mcps(&self, channel_id: &str, names: &[String]) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        let val = if names.is_empty() {
            None
        } else {
            Some(serde_json::to_string(names).unwrap())
        };
        conn.execute(
            "UPDATE projects SET disabled_mcps = ?1 WHERE channel_id = ?2",
            params![val, channel_id],
        )?;
        Ok(())
    }
}
