mod manager;
pub use manager::SessionManager;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Creating,
    Authenticating,
    Active,
    Sleeping,
    Terminated,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Authenticating => "authenticating",
            Self::Active => "active",
            Self::Sleeping => "sleeping",
            Self::Terminated => "terminated",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "creating" => Self::Creating,
            "authenticating" => Self::Authenticating,
            "active" => Self::Active,
            "sleeping" => Self::Sleeping,
            "terminated" => Self::Terminated,
            _ => Self::Terminated,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub channel_id: String,
    pub thread_ts: String,
    pub machine_id: Option<String>,
    pub oauth_token: Option<String>,
    pub pending_prompt: Option<String>,
    pub state: SessionState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
