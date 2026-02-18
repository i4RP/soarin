use anyhow::Result;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::{Session, SessionState};

#[derive(Clone)]
pub struct SessionManager {
    pool: SqlitePool,
    max_concurrent: usize,
}

impl SessionManager {
    pub fn new(pool: SqlitePool, max_concurrent: usize) -> Self {
        Self {
            pool,
            max_concurrent,
        }
    }

    pub async fn init_db(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                thread_ts TEXT NOT NULL,
                machine_id TEXT,
                state TEXT NOT NULL DEFAULT 'creating',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn active_session_count(&self) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sessions WHERE state IN ('creating', 'authenticating', 'active', 'sleeping')",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn can_create_session(&self) -> Result<bool> {
        let count = self.active_session_count().await?;
        Ok((count as usize) < self.max_concurrent)
    }

    pub async fn create_session(
        &self,
        user_id: &str,
        channel_id: &str,
        thread_ts: &str,
    ) -> Result<Session> {
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO sessions (id, user_id, channel_id, thread_ts, state, created_at, updated_at)
            VALUES (?, ?, ?, ?, 'creating', ?, ?)
            "#,
        )
        .bind(&id)
        .bind(user_id)
        .bind(channel_id)
        .bind(thread_ts)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        self.get_session(&id).await
    }

    pub async fn get_session(&self, id: &str) -> Result<Session> {
        let row = sqlx::query_as::<_, (String, String, String, String, Option<String>, String, String, String)>(
            "SELECT id, user_id, channel_id, thread_ts, machine_id, state, created_at, updated_at FROM sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;

        Ok(Session {
            id: row.0,
            user_id: row.1,
            channel_id: row.2,
            thread_ts: row.3,
            machine_id: row.4,
            state: SessionState::from_str(&row.5),
            created_at: chrono::DateTime::parse_from_rfc3339(&row.6)?.with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339(&row.7)?.with_timezone(&chrono::Utc),
        })
    }

    pub async fn find_session_by_thread(
        &self,
        channel_id: &str,
        thread_ts: &str,
    ) -> Result<Option<Session>> {
        let row = sqlx::query_as::<_, (String, String, String, String, Option<String>, String, String, String)>(
            "SELECT id, user_id, channel_id, thread_ts, machine_id, state, created_at, updated_at FROM sessions WHERE channel_id = ? AND thread_ts = ? AND state != 'terminated'",
        )
        .bind(channel_id)
        .bind(thread_ts)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(Session {
                id: r.0,
                user_id: r.1,
                channel_id: r.2,
                thread_ts: r.3,
                machine_id: r.4,
                state: SessionState::from_str(&r.5),
                created_at: chrono::DateTime::parse_from_rfc3339(&r.6)?.with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&r.7)?.with_timezone(&chrono::Utc),
            })),
            None => Ok(None),
        }
    }

    pub async fn update_state(&self, id: &str, state: SessionState) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE sessions SET state = ?, updated_at = ? WHERE id = ?")
            .bind(state.as_str())
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_machine_id(&self, id: &str, machine_id: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE sessions SET machine_id = ?, updated_at = ? WHERE id = ?")
            .bind(machine_id)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_active_sessions(&self) -> Result<Vec<Session>> {
        let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, String, String, String)>(
            "SELECT id, user_id, channel_id, thread_ts, machine_id, state, created_at, updated_at FROM sessions WHERE state != 'terminated' ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let sessions = rows
            .into_iter()
            .filter_map(|r| {
                Some(Session {
                    id: r.0,
                    user_id: r.1,
                    channel_id: r.2,
                    thread_ts: r.3,
                    machine_id: r.4,
                    state: SessionState::from_str(&r.5),
                    created_at: chrono::DateTime::parse_from_rfc3339(&r.6).ok()?.with_timezone(&chrono::Utc),
                    updated_at: chrono::DateTime::parse_from_rfc3339(&r.7).ok()?.with_timezone(&chrono::Utc),
                })
            })
            .collect();

        Ok(sessions)
    }
}
