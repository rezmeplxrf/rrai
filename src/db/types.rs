use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Online,
    Offline,
    Waiting,
    Idle,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Waiting => "waiting",
            Self::Idle => "idle",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "online" => Self::Online,
            "waiting" => Self::Waiting,
            "idle" => Self::Idle,
            _ => Self::Offline,
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    pub channel_id: String,
    pub project_path: String,
    pub guild_id: String,
    pub auto_approve: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub channel_id: String,
    pub session_id: Option<String>,
    pub status: SessionStatus,
    pub last_activity: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SessionWithProject {
    pub session: Session,
    pub project_path: String,
}
