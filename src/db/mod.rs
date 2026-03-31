pub mod types;

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Arc;

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
                status TEXT DEFAULT 'offline',
                last_activity TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            );

",
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // --- Project queries ---

    pub fn register_project(&self, channel_id: &str, project_path: &str, guild_id: &str) {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO projects (channel_id, project_path, guild_id) VALUES (?1, ?2, ?3)",
            params![channel_id, project_path, guild_id],
        )
        .ok();
    }

    pub fn unregister_project(&self, channel_id: &str) {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions WHERE channel_id = ?1", params![channel_id]).ok();
        conn.execute("DELETE FROM projects WHERE channel_id = ?1", params![channel_id]).ok();
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
        .ok()
        .flatten()
    }

    pub fn get_all_projects(&self, guild_id: &str) -> Vec<Project> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT channel_id, project_path, guild_id, auto_approve, created_at FROM projects WHERE guild_id = ?1")
            .unwrap();
        stmt.query_map(params![guild_id], |row| {
            Ok(Project {
                channel_id: row.get(0)?,
                project_path: row.get(1)?,
                guild_id: row.get(2)?,
                auto_approve: row.get::<_, i32>(3)? != 0,
                created_at: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn set_auto_approve(&self, channel_id: &str, auto_approve: bool) {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE projects SET auto_approve = ?1 WHERE channel_id = ?2",
            params![auto_approve as i32, channel_id],
        )
        .ok();
    }

    // --- Session queries ---

    pub fn upsert_session(
        &self,
        id: &str,
        channel_id: &str,
        session_id: Option<&str>,
        status: SessionStatus,
    ) {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO sessions (id, channel_id, session_id, status, last_activity) VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            params![id, channel_id, session_id, status.as_str()],
        )
        .ok();
    }

    pub fn get_session(&self, channel_id: &str) -> Option<Session> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, channel_id, session_id, status, last_activity, created_at FROM sessions WHERE channel_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![channel_id],
            |row| {
                Ok(Session {
                    id: row.get(0)?,
                    channel_id: row.get(1)?,
                    session_id: row.get(2)?,
                    status: SessionStatus::parse(&row.get::<_, String>(3)?),
                    last_activity: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn clear_session(&self, channel_id: &str) {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions WHERE channel_id = ?1", params![channel_id]).ok();
    }

    pub fn update_session_status(&self, channel_id: &str, status: SessionStatus) {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET status = ?1, last_activity = datetime('now') WHERE channel_id = ?2",
            params![status.as_str(), channel_id],
        )
        .ok();
    }

    pub fn get_all_sessions(&self, guild_id: &str) -> Vec<SessionWithProject> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT s.id, s.channel_id, s.session_id, s.status, s.last_activity, s.created_at, p.project_path
                 FROM sessions s JOIN projects p ON s.channel_id = p.channel_id
                 WHERE p.guild_id = ?1",
            )
            .unwrap();
        stmt.query_map(params![guild_id], |row| {
            Ok(SessionWithProject {
                session: Session {
                    id: row.get(0)?,
                    channel_id: row.get(1)?,
                    session_id: row.get(2)?,
                    status: SessionStatus::parse(&row.get::<_, String>(3)?),
                    last_activity: row.get(4)?,
                    created_at: row.get(5)?,
                },
                project_path: row.get(6)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // --- Model per channel ---

    pub fn set_model(&self, channel_id: &str, model: Option<&str>) {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE projects SET model = ?1 WHERE channel_id = ?2",
            params![model, channel_id],
        )
        .ok();
    }

    pub fn get_model(&self, channel_id: &str) -> Option<String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT model FROM projects WHERE channel_id = ?1",
            params![channel_id],
            |row| row.get::<_, Option<String>>(0),
        )
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
            .ok()
            .flatten();
        match raw {
            Some(s) => serde_json::from_str(&s).unwrap_or_default(),
            None => vec![],
        }
    }

    pub fn set_disabled_mcps(&self, channel_id: &str, names: &[String]) {
        let conn = self.conn.lock();
        let val = if names.is_empty() {
            None
        } else {
            Some(serde_json::to_string(names).unwrap())
        };
        conn.execute(
            "UPDATE projects SET disabled_mcps = ?1 WHERE channel_id = ?2",
            params![val, channel_id],
        )
        .ok();
    }
}
