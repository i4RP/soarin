use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;
use crate::session::SessionState;

pub async fn auth_start_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let session = match state.sessions.get_session(&session_id).await {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::NOT_FOUND, Html("Session not found".to_string())).into_response();
        }
    };

    if session.state != SessionState::Authenticating {
        return (
            StatusCode::BAD_REQUEST,
            Html(format!("Session is in {:?} state, not authenticating", session.state)),
        )
            .into_response();
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Soarin - Claude Code Authentication</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
        }}
        .container {{
            background: white;
            border-radius: 16px;
            padding: 48px;
            max-width: 480px;
            width: 90%;
            box-shadow: 0 20px 60px rgba(0,0,0,0.3);
            text-align: center;
        }}
        h1 {{
            font-size: 24px;
            color: #1a1a2e;
            margin-bottom: 8px;
        }}
        .subtitle {{
            color: #666;
            margin-bottom: 32px;
            font-size: 14px;
        }}
        .session-info {{
            background: #f5f5f5;
            border-radius: 8px;
            padding: 16px;
            margin-bottom: 24px;
            font-size: 13px;
            color: #555;
        }}
        .btn {{
            display: inline-block;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            padding: 14px 36px;
            border-radius: 8px;
            text-decoration: none;
            font-weight: 600;
            font-size: 16px;
            border: none;
            cursor: pointer;
            transition: transform 0.2s, box-shadow 0.2s;
        }}
        .btn:hover {{
            transform: translateY(-2px);
            box-shadow: 0 8px 25px rgba(102,126,234,0.4);
        }}
        .btn:disabled {{
            opacity: 0.6;
            cursor: not-allowed;
            transform: none;
        }}
        .status {{
            margin-top: 24px;
            font-size: 14px;
            color: #666;
        }}
        .success {{
            color: #22c55e;
            font-weight: 600;
        }}
        .error {{
            color: #ef4444;
            font-weight: 600;
        }}
        .spinner {{
            display: inline-block;
            width: 20px;
            height: 20px;
            border: 3px solid #e0e0e0;
            border-top-color: #667eea;
            border-radius: 50%;
            animation: spin 0.8s linear infinite;
            margin-right: 8px;
            vertical-align: middle;
        }}
        @keyframes spin {{
            to {{ transform: rotate(360deg); }}
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Soarin</h1>
        <p class="subtitle">Claude Code Authentication</p>
        <div class="session-info">
            Session ID: <code>{session_id}</code>
        </div>
        <p style="margin-bottom: 24px; color: #444; font-size: 14px; line-height: 1.6;">
            Click below to authenticate with your Claude Code subscription.
            This will connect your Anthropic account to this Soarin session.
        </p>
        <button class="btn" id="authBtn" onclick="startAuth()">
            Authenticate with Claude
        </button>
        <div class="status" id="status"></div>
    </div>
    <script>
        const sessionId = "{session_id}";
        const backendUrl = "{backend_url}";

        async function startAuth() {{
            const btn = document.getElementById('authBtn');
            const status = document.getElementById('status');
            btn.disabled = true;
            status.innerHTML = '<span class="spinner"></span> Initiating authentication...';

            try {{
                const resp = await fetch(`${{backendUrl}}/auth/claude/${{sessionId}}/initiate`, {{
                    method: 'POST',
                }});
                const data = await resp.json();

                if (data.auth_url) {{
                    status.innerHTML = '<span class="spinner"></span> Waiting for authentication... A new tab will open.';
                    window.open(data.auth_url, '_blank');
                    pollStatus();
                }} else if (data.status === 'ready') {{
                    status.innerHTML = '<span class="success">Authentication complete! You can close this tab.</span>';
                }} else {{
                    status.innerHTML = `<span class="error">Error: ${{data.error || 'Unknown error'}}</span>`;
                    btn.disabled = false;
                }}
            }} catch (e) {{
                status.innerHTML = `<span class="error">Connection error: ${{e.message}}</span>`;
                btn.disabled = false;
            }}
        }}

        async function pollStatus() {{
            const status = document.getElementById('status');
            for (let i = 0; i < 60; i++) {{
                await new Promise(r => setTimeout(r, 3000));
                try {{
                    const resp = await fetch(`${{backendUrl}}/auth/claude/${{sessionId}}/status`);
                    const data = await resp.json();
                    if (data.authenticated) {{
                        status.innerHTML = '<span class="success">Authentication complete! Soarin is ready. You can close this tab and return to Slack.</span>';
                        return;
                    }}
                }} catch (e) {{
                    // continue polling
                }}
            }}
            status.innerHTML = '<span class="error">Authentication timed out. Please try again.</span>';
            document.getElementById('authBtn').disabled = false;
        }}
    </script>
</body>
</html>"#,
        session_id = session_id,
        backend_url = state.config.base_url,
    );

    (StatusCode::OK, Html(html)).into_response()
}

pub async fn auth_callback_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let session = match state.sessions.get_session(&session_id).await {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "session not found"}))).into_response();
        }
    };

    match state
        .sessions
        .update_state(&session.id, SessionState::Active)
        .await
    {
        Ok(_) => {
            let _ = state
                .slack
                .post_message(
                    &session.channel_id,
                    ":white_check_mark: Claude Code authenticated! Soarin is now ready. Send messages in this thread to start working.",
                    Some(&session.thread_ts),
                )
                .await;

            (StatusCode::OK, Json(json!({"status": "ready"}))).into_response()
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response()
        }
    }
}

pub async fn auth_status_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let session = match state.sessions.get_session(&session_id).await {
        Ok(s) => s,
        Err(_) => {
            return Json(json!({"error": "session not found", "authenticated": false}));
        }
    };

    let authenticated = session.state == SessionState::Active;
    Json(json!({
        "authenticated": authenticated,
        "state": format!("{:?}", session.state),
    }))
}
