use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub slack_bot_token: String,
    pub slack_signing_secret: String,
    pub flyio_api_token: String,
    pub flyio_app_name: String,
    pub flyio_worker_image: String,
    pub flyio_region: String,
    pub base_url: String,
    pub max_concurrent_sessions: usize,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse()?,
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:///data/soarin.db?mode=rwc".into()),
            slack_bot_token: std::env::var("SLACK_BOT_TOKEN")?,
            slack_signing_secret: std::env::var("SLACK_SIGNING_SECRET")?,
            flyio_api_token: std::env::var("FLYIO_API_TOKEN")?,
            flyio_app_name: std::env::var("FLYIO_APP_NAME")
                .unwrap_or_else(|_| "soarin-workers".into()),
            flyio_worker_image: std::env::var("FLYIO_WORKER_IMAGE")
                .unwrap_or_else(|_| "ghcr.io/anthropics/claude-code:latest".into()),
            flyio_region: std::env::var("FLYIO_REGION")
                .unwrap_or_else(|_| "nrt".into()),
            base_url: std::env::var("BASE_URL")?,
            max_concurrent_sessions: std::env::var("MAX_CONCURRENT_SESSIONS")
                .unwrap_or_else(|_| "2".into())
                .parse()?,
        })
    }
}
