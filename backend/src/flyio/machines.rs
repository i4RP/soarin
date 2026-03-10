use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct FlyMachineClient {
    client: Client,
    api_token: String,
    app_name: String,
    worker_image: String,
    region: String,
}

#[derive(Debug, Serialize)]
struct CreateMachineRequest {
    name: String,
    region: String,
    config: MachineConfig,
}

#[derive(Debug, Serialize)]
struct MachineConfig {
    image: String,
    env: std::collections::HashMap<String, String>,
    guest: GuestConfig,
    services: Vec<MachineService>,
    auto_destroy: bool,
}

#[derive(Debug, Serialize)]
struct GuestConfig {
    cpu_kind: String,
    cpus: u32,
    memory_mb: u32,
}

#[derive(Debug, Serialize)]
struct MachineService {
    protocol: String,
    internal_port: u16,
    ports: Vec<ServicePort>,
}

#[derive(Debug, Serialize)]
struct ServicePort {
    port: u16,
    handlers: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct MachineResponse {
    pub id: String,
    pub name: Option<String>,
    pub state: Option<String>,
    pub instance_id: Option<String>,
    pub private_ip: Option<String>,
}

impl FlyMachineClient {
    pub fn new(
        api_token: String,
        app_name: String,
        worker_image: String,
        region: String,
    ) -> Self {
        Self {
            client: Client::new(),
            api_token,
            app_name,
            worker_image,
            region,
        }
    }

    fn base_url(&self) -> String {
        format!("https://api.machines.dev/v1/apps/{}", self.app_name)
    }

    pub async fn create_machine(
        &self,
        session_id: &str,
        env_vars: std::collections::HashMap<String, String>,
    ) -> Result<MachineResponse> {
        let mut env = env_vars;
        env.insert("SESSION_ID".into(), session_id.into());

        let body = CreateMachineRequest {
            name: format!("soarin-worker-{}", &session_id[..8]),
            region: self.region.clone(),
            config: MachineConfig {
                image: self.worker_image.clone(),
                env,
                guest: GuestConfig {
                    cpu_kind: "shared".into(),
                    cpus: 2,
                    memory_mb: 2048,
                },
                services: vec![MachineService {
                    protocol: "tcp".into(),
                    internal_port: 3000,
                    ports: vec![ServicePort {
                        port: 443,
                        handlers: vec!["tls".into(), "http".into()],
                    }],
                }],
                auto_destroy: true,
            },
        };

        let resp = self
            .client
            .post(format!("{}/machines", self.base_url()))
            .header("Authorization", format!("Bearer {}", self.api_token))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to create machine: {} - {}", status, err_body);
        }

        let machine: MachineResponse = resp.json().await?;
        Ok(machine)
    }

    pub async fn start_machine(&self, machine_id: &str) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/machines/{}/start", self.base_url(), machine_id))
            .header("Authorization", format!("Bearer {}", self.api_token))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to start machine: {}", err_body);
        }

        Ok(())
    }

    pub async fn stop_machine(&self, machine_id: &str) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/machines/{}/stop", self.base_url(), machine_id))
            .header("Authorization", format!("Bearer {}", self.api_token))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to stop machine: {}", err_body);
        }

        Ok(())
    }

    pub async fn destroy_machine(&self, machine_id: &str) -> Result<()> {
        let resp = self
            .client
            .delete(format!("{}/machines/{}?force=true", self.base_url(), machine_id))
            .header("Authorization", format!("Bearer {}", self.api_token))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to destroy machine: {}", err_body);
        }

        Ok(())
    }

    pub async fn get_machine(&self, machine_id: &str) -> Result<MachineResponse> {
        let resp = self
            .client
            .get(format!("{}/machines/{}", self.base_url(), machine_id))
            .header("Authorization", format!("Bearer {}", self.api_token))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to get machine: {}", err_body);
        }

        let machine: MachineResponse = resp.json().await?;
        Ok(machine)
    }
}
