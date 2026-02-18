mod config;
mod session;
mod slack;
mod flyio;
mod auth;

use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};
use sqlx::sqlite::SqlitePoolOptions;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use config::Config;
use flyio::FlyMachineClient;
use session::SessionManager;
use slack::SlackClient;

pub struct AppState {
    pub config: Config,
    pub sessions: SessionManager,
    pub slack: SlackClient,
    pub fly_client: FlyMachineClient,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    dotenvy::dotenv().ok();

    let config = Config::from_env()?;
    tracing::info!("Starting Soarin backend on {}:{}", config.host, config.port);

    let db_path = &config.database_url;
    if db_path.contains("/data/") {
        tokio::fs::create_dir_all("/data").await.ok();
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await?;

    let sessions = SessionManager::new(pool, config.max_concurrent_sessions);
    sessions.init_db().await?;

    let slack = SlackClient::new(config.slack_bot_token.clone());
    let fly_client = FlyMachineClient::new(
        config.flyio_api_token.clone(),
        config.flyio_app_name.clone(),
        config.flyio_worker_image.clone(),
        config.flyio_region.clone(),
    );

    let state = Arc::new(AppState {
        config: config.clone(),
        sessions,
        slack,
        fly_client,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/slack/events", post(slack::slack_events_handler))
        .route("/slack/commands", post(slack::slack_commands_handler))
        .route("/auth/claude/{session_id}", get(auth::auth_start_handler))
        .route(
            "/auth/claude/{session_id}/initiate",
            post(auth::auth_callback_handler),
        )
        .route(
            "/auth/claude/{session_id}/status",
            get(auth::auth_status_handler),
        )
        .route("/api/sessions", get(api_sessions_handler))
        .route(
            "/api/worker/callback",
            post(worker_callback_handler),
        )
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_handler() -> &'static str {
    "ok"
}

async fn api_sessions_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    match state.sessions.list_active_sessions().await {
        Ok(sessions) => axum::Json(serde_json::json!({ "sessions": sessions })),
        Err(e) => axum::Json(serde_json::json!({ "error": e.to_string() })),
    }
}

async fn worker_callback_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    let session_id = body["session_id"].as_str().unwrap_or("");
    let message = body["message"].as_str().unwrap_or("");
    let channel_id = body["channel_id"].as_str().unwrap_or("");
    let thread_ts = body["thread_ts"].as_str().unwrap_or("");

    if !session_id.is_empty() && !message.is_empty() {
        let _ = state
            .slack
            .post_message(channel_id, message, Some(thread_ts))
            .await;
    }

    axum::Json(serde_json::json!({"ok": true}))
}
