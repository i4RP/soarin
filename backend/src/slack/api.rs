use anyhow::Result;
use reqwest::Client;
use serde_json::json;

#[derive(Clone)]
pub struct SlackClient {
    client: Client,
    bot_token: String,
}

impl SlackClient {
    pub fn new(bot_token: String) -> Self {
        Self {
            client: Client::new(),
            bot_token,
        }
    }

    pub async fn post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut body = json!({
            "channel": channel,
            "text": text,
        });

        if let Some(ts) = thread_ts {
            body["thread_ts"] = json!(ts);
        }

        let resp = self
            .client
            .post("https://slack.com/api/chat.postMessage")
            .header("Authorization", format!("Bearer {}", self.bot_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let data: serde_json::Value = resp.json().await?;
        if data["ok"].as_bool() != Some(true) {
            anyhow::bail!("Slack API error: {}", data);
        }

        Ok(data)
    }

    pub async fn post_message_get_ts(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<String> {
        let data = self.post_message(channel, text, thread_ts).await?;
        let ts = data["ts"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No ts in response"))?
            .to_string();
        Ok(ts)
    }

    pub async fn update_message(
        &self,
        channel: &str,
        ts: &str,
        text: &str,
    ) -> Result<()> {
        let body = json!({
            "channel": channel,
            "ts": ts,
            "text": text,
        });

        let resp = self
            .client
            .post("https://slack.com/api/chat.update")
            .header("Authorization", format!("Bearer {}", self.bot_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let data: serde_json::Value = resp.json().await?;
        if data["ok"].as_bool() != Some(true) {
            anyhow::bail!("Slack API error: {}", data);
        }

        Ok(())
    }
}
