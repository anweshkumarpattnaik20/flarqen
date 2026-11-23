#![allow(dead_code)]
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxStatus {
    Creating,
    Starting,
    Running,
    Stopping,
    Stopped,
    Resetting,
    Error,
    Unknown,
}

impl SandboxStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Resetting => "resetting",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "creating" => Self::Creating,
            "starting" => Self::Starting,
            "running" => Self::Running,
            "stopping" => Self::Stopping,
            "stopped" => Self::Stopped,
            "resetting" => Self::Resetting,
            "error" => Self::Error,
            _ => Self::Unknown,
        }
    }

    /// Allowed lifecycle transitions. Anything not listed is rejected.
    pub fn can_transition_to(self, next: Self) -> bool {
        use SandboxStatus::*;
        matches!(
            (self, next),
            (Creating, Stopped)
                | (Creating, Error)
                | (Stopped, Starting)
                | (Stopped, Resetting)
                | (Starting, Running)
                | (Starting, Stopped)
                | (Starting, Error)
                | (Running, Stopping)
                | (Running, Stopped) // unexpected exit found during reconciliation
                | (Running, Error)
                | (Stopping, Stopped)
                | (Stopping, Error)
                | (Resetting, Stopped)
                | (Resetting, Error)
                | (Error, Stopped)
                | (Unknown, Stopped)
                | (Unknown, Running)
                | (Unknown, Error)
                | (_, Unknown)
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OsImage {
    pub id: String,
    pub name: String,
    pub version: String,
    pub image_path: String,
    pub image_format: String,
    pub checksum: Option<String>,
    pub verified: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sandbox {
    pub id: String,
    pub name: String,
    pub description: String,
    pub operating_system: String,
    pub os_version: String,
    pub base_image_id: String,
    pub overlay_path: String,
    pub cpu_cores: u32,
    pub ram_mb: i64,
    pub disk_capacity_gb: i64,
    pub status: SandboxStatus,
    pub process_id: Option<u32>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_started_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityEntry {
    pub id: i64,
    pub operation: String,
    pub sandbox_id: Option<String>,
    pub status: String,
    pub message: String,
    pub timestamp: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use SandboxStatus::*;

    #[test]
    fn valid_transitions_are_allowed() {
        assert!(Stopped.can_transition_to(Starting));
        assert!(Starting.can_transition_to(Running));
        assert!(Running.can_transition_to(Stopping));
        assert!(Stopping.can_transition_to(Stopped));
        assert!(Stopped.can_transition_to(Resetting));
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        assert!(!Stopped.can_transition_to(Running)); // must go through Starting
        assert!(!Running.can_transition_to(Resetting)); // must stop first
        assert!(!Running.can_transition_to(Starting));
        assert!(!Creating.can_transition_to(Running));
    }

    #[test]
    fn status_round_trips_through_text() {
        for s in [Creating, Starting, Running, Stopping, Stopped, Resetting, Error, Unknown] {
            assert_eq!(SandboxStatus::parse(s.as_str()), s);
        }
        assert_eq!(SandboxStatus::parse("garbage"), Unknown);
    }
}