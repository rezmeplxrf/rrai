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

    pub fn parse(s: &str) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_status_roundtrip() {
        for status in [
            SessionStatus::Online,
            SessionStatus::Offline,
            SessionStatus::Waiting,
            SessionStatus::Idle,
        ] {
            let s = status.as_str();
            let parsed = SessionStatus::parse(s);
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn session_status_as_str() {
        assert_eq!(SessionStatus::Online.as_str(), "online");
        assert_eq!(SessionStatus::Offline.as_str(), "offline");
        assert_eq!(SessionStatus::Waiting.as_str(), "waiting");
        assert_eq!(SessionStatus::Idle.as_str(), "idle");
    }

    #[test]
    fn session_status_unknown_defaults_to_offline() {
        assert_eq!(SessionStatus::parse("garbage"), SessionStatus::Offline);
        assert_eq!(SessionStatus::parse(""), SessionStatus::Offline);
        assert_eq!(SessionStatus::parse("ONLINE"), SessionStatus::Offline);
    }

    #[test]
    fn session_status_display() {
        assert_eq!(format!("{}", SessionStatus::Online), "online");
        assert_eq!(format!("{}", SessionStatus::Idle), "idle");
    }

    #[test]
    fn session_status_serde_roundtrip() {
        let status = SessionStatus::Waiting;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"waiting\"");
        let parsed: SessionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }

    #[test]
    fn session_status_serde_all_variants() {
        for status in [
            SessionStatus::Online,
            SessionStatus::Offline,
            SessionStatus::Waiting,
            SessionStatus::Idle,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: SessionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }
}
