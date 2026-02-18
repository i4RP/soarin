use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;
use crate::session::SessionState;

type HmacSha256 = Hmac<Sha256>;

fn verify_slack_signature(
    signing_secret: &str,
    timestamp: &str,
    body: &str,
    signature: &str,
) -> bool {
    let sig_basestring = format!("v0:{}:{}", timestamp, body);
    let mut mac = match HmacSha256::new_from_slice(signing_secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(sig_basestring.as_bytes());
    let result = mac.finalize();
    let computed = format!("v0={}", hex::encode(result.into_bytes()));
    computed == signature
}

pub async fn slack_events_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let timestamp = headers
        .get("X-Slack-Request-Timestamp")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let signature = headers
        .get("X-Slack-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !verify_slack_signature(&state.config.slack_signing_secret, timestamp, &body, signature) {
        tracing::warn!("Invalid Slack signature");
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid signature"}))).into_response();
    }

    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to parse Slack event: {}", e);
            return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid json"}))).into_response();
        }
    };

    if payload["type"].as_str() == Some("url_verification") {
        let challenge = payload["challenge"].as_str().unwrap_or("");
        return (StatusCode::OK, Json(json!({"challenge": challenge}))).into_response();
    }

    if payload["type"].as_str() == Some("event_callback") {
        let event = &payload["event"];
        let event_type = event["type"].as_str().unwrap_or("");

        match event_type {
            "app_mention" => {
                let state_clone = state.clone();
                let event_clone = event.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_app_mention(state_clone, event_clone).await {
                        tracing::error!("Error handling app_mention: {}", e);
                    }
                });
            }
            "message" => {
                if event.get("bot_id").is_none() && event.get("subtype").is_none() {
                    if let Some(thread_ts) = event["thread_ts"].as_str() {
                        let state_clone = state.clone();
                        let event_clone = event.clone();
                        let thread_ts = thread_ts.to_string();
                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_thread_message(state_clone, event_clone, &thread_ts).await
                            {
                                tracing::error!("Error handling thread message: {}", e);
                            }
                        });
                    }
                }
            }
            _ => {
                tracing::debug!("Unhandled event type: {}", event_type);
            }
        }
    }

    (StatusCode::OK, Json(json!({"ok": true}))).into_response()
}

async fn handle_app_mention(state: Arc<AppState>, event: Value) -> anyhow::Result<()> {
    let user_id = event["user"].as_str().unwrap_or("");
    let channel_id = event["channel"].as_str().unwrap_or("");
    let text = event["text"].as_str().unwrap_or("");
    let event_ts = event["ts"].as_str().unwrap_or("");

    let prompt = extract_prompt(text);
    tracing::info!(
        "App mention from user={} channel={} prompt={}",
        user_id,
        channel_id,
        prompt
    );

    if !state.sessions.can_create_session().await? {
        state
            .slack
            .post_message(
                channel_id,
                ":warning: Maximum concurrent sessions reached (2). Please wait for an existing session to finish or use `/sleep` to free up a slot.",
                Some(event_ts),
            )
            .await?;
        return Ok(());
    }

    let reply_ts = state
        .slack
        .post_message_get_ts(
            channel_id,
            ":rocket: Starting new Soarin session... Setting up your development environment.",
            Some(event_ts),
        )
        .await?;

    let session = state
        .sessions
        .create_session(user_id, channel_id, event_ts)
        .await?;

    tracing::info!("Created session: {}", session.id);

    let mut env_vars = std::collections::HashMap::new();
    env_vars.insert("BACKEND_URL".into(), state.config.base_url.clone());
    env_vars.insert("SLACK_CHANNEL".into(), channel_id.to_string());
    env_vars.insert("SLACK_THREAD_TS".into(), event_ts.to_string());

    match state.fly_client.create_machine(&session.id, env_vars).await {
        Ok(machine) => {
            state
                .sessions
                .set_machine_id(&session.id, &machine.id)
                .await?;

            tracing::info!(
                "Machine {} created for session {}",
                machine.id,
                session.id
            );

            state
                .slack
                .update_message(
                    channel_id,
                    &reply_ts,
                    ":white_check_mark: VM started! Waiting for worker to be ready...",
                )
                .await?;

            let mut worker_ready = false;
            for attempt in 0..15 {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                match state.fly_client.get_machine(&machine.id).await {
                    Ok(m) => {
                        if m.state.as_deref() == Some("started") && m.private_ip.is_some() {
                            worker_ready = true;
                            tracing::info!("Worker ready on attempt {}", attempt + 1);
                            break;
                        }
                        tracing::info!("Worker state: {:?} (attempt {})", m.state, attempt + 1);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to check machine state: {}", e);
                    }
                }
            }

            if worker_ready {
                state
                    .sessions
                    .update_state(&session.id, SessionState::Active)
                    .await?;

                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                state
                    .slack
                    .update_message(
                        channel_id,
                        &reply_ts,
                        ":white_check_mark: VM ready! Processing your request...",
                    )
                    .await?;

                send_prompt_to_worker(&state, &session.id, user_id, &prompt).await?;
            } else {
                state
                    .slack
                    .update_message(
                        channel_id,
                        &reply_ts,
                        ":warning: VM started but worker is taking longer than expected. Try sending your message again in the thread.",
                    )
                    .await?;
                state
                    .sessions
                    .update_state(&session.id, SessionState::Active)
                    .await?;
            }
        }
        Err(e) => {
            tracing::error!("Failed to create machine: {}", e);
            state
                .sessions
                .update_state(&session.id, SessionState::Terminated)
                .await?;
            state
                .slack
                .update_message(
                    channel_id,
                    &reply_ts,
                    &format!(":x: Failed to start VM: {}", e),
                )
                .await?;
        }
    }

    Ok(())
}

async fn handle_thread_message(
    state: Arc<AppState>,
    event: Value,
    thread_ts: &str,
) -> anyhow::Result<()> {
    let channel_id = event["channel"].as_str().unwrap_or("");
    let text = event["text"].as_str().unwrap_or("");
    let user_id = event["user"].as_str().unwrap_or("");

    let session = state
        .sessions
        .find_session_by_thread(channel_id, thread_ts)
        .await?;

    let session = match session {
        Some(s) => s,
        None => return Ok(()),
    };

    match session.state {
        SessionState::Sleeping => {
            tracing::info!("Waking session {} for thread message", session.id);
            state
                .slack
                .post_message(
                    channel_id,
                    ":sunrise: Waking up Soarin... Please wait.",
                    Some(thread_ts),
                )
                .await?;

            if let Some(machine_id) = &session.machine_id {
                state.fly_client.start_machine(machine_id).await?;
                state
                    .sessions
                    .update_state(&session.id, SessionState::Active)
                    .await?;

                send_prompt_to_worker(&state, &session.id, user_id, text).await?;
            }
        }
        SessionState::Active => {
            send_prompt_to_worker(&state, &session.id, user_id, text).await?;
        }
        SessionState::Authenticating => {
            state
                .slack
                .post_message(
                    channel_id,
                    ":hourglass: Please complete Claude Code authentication first.",
                    Some(thread_ts),
                )
                .await?;
        }
        _ => {}
    }

    Ok(())
}

async fn send_prompt_to_worker(
    state: &Arc<AppState>,
    session_id: &str,
    user_id: &str,
    prompt: &str,
) -> anyhow::Result<()> {
    let session = state.sessions.get_session(session_id).await?;
    let machine_id = session
        .machine_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No machine ID for session"))?;

    let machine = state.fly_client.get_machine(machine_id).await?;
    let worker_url = if let Some(ip) = &machine.private_ip {
        format!("http://[{}]:3000", ip)
    } else {
        format!(
            "http://{}.flycast:3000",
            machine_id
        )
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/prompt", worker_url))
        .json(&serde_json::json!({
            "session_id": session_id,
            "user_id": user_id,
            "prompt": prompt,
            "channel_id": session.channel_id,
            "thread_ts": session.thread_ts,
        }))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            tracing::info!("Prompt sent to worker for session {}", session_id);
        }
        Ok(r) => {
            let err = r.text().await.unwrap_or_default();
            tracing::error!("Worker error: {}", err);
        }
        Err(e) => {
            tracing::error!("Failed to reach worker: {}", e);
            state
                .slack
                .post_message(
                    &session.channel_id,
                    ":warning: Worker VM is not responding. It may still be starting up. Please try again in a moment.",
                    Some(&session.thread_ts),
                )
                .await?;
        }
    }

    Ok(())
}

pub async fn slack_commands_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let params: std::collections::HashMap<String, String> =
        serde_urlencoded::from_str(&body).unwrap_or_default();

    let timestamp = headers
        .get("X-Slack-Request-Timestamp")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let signature = headers
        .get("X-Slack-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !verify_slack_signature(&state.config.slack_signing_secret, timestamp, &body, signature) {
        return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
    }

    let command = params.get("command").map(|s| s.as_str()).unwrap_or("");
    let channel_id = params.get("channel_id").map(|s| s.as_str()).unwrap_or("");

    match command {
        "/sleep" => {
            let state_clone = state.clone();
            let channel_id = channel_id.to_string();
            tokio::spawn(async move {
                if let Err(e) = handle_sleep_command(&state_clone, &channel_id).await {
                    tracing::error!("Error handling /sleep: {}", e);
                }
            });

            (StatusCode::OK, ":zzz: Putting Soarin to sleep...").into_response()
        }
        _ => (StatusCode::OK, "Unknown command").into_response(),
    }
}

async fn handle_sleep_command(state: &Arc<AppState>, channel_id: &str) -> anyhow::Result<()> {
    let sessions = state.sessions.list_active_sessions().await?;
    let session = sessions
        .iter()
        .find(|s| s.channel_id == channel_id && s.state == SessionState::Active);

    let session = match session {
        Some(s) => s.clone(),
        None => {
            state
                .slack
                .post_message(channel_id, ":warning: No active session found in this channel.", None)
                .await?;
            return Ok(());
        }
    };

    if let Some(machine_id) = &session.machine_id {
        state.fly_client.stop_machine(machine_id).await?;
        state
            .sessions
            .update_state(&session.id, SessionState::Sleeping)
            .await?;
        state
            .slack
            .post_message(
                channel_id,
                ":zzz: Soarin is now sleeping. Send a message in the thread to wake it up.",
                Some(&session.thread_ts),
            )
            .await?;
    }

    Ok(())
}

fn extract_prompt(text: &str) -> String {
    let re = regex::Regex::new(r"<@[A-Z0-9]+>\s*(.*)").unwrap_or_else(|_| {
        regex::Regex::new(r".*").unwrap()
    });
    match re.captures(text) {
        Some(caps) => caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
        None => text.trim().to_string(),
    }
}
