use anyhow::Result;
use chrono::Utc;
use reqwest::Client;
use rtools_domain::Task;
use rtools_sync_proto::{ChangePage, OperationBatch, OperationResponse, PairRequest, PairResponse};
use rtools_sync_proto::{DeviceInfo, PairingCodeResponse};
use std::time::Duration;

pub struct SyncClient {
    client: Client,
    base_url: String,
    token: Option<String>,
}

impl SyncClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        Ok(Self {
            client: Client::builder().timeout(Duration::from_secs(20)).build()?,
            base_url: base_url.into().trim_end_matches('/').into(),
            token: None,
        })
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub async fn pair(&self, code: &str, device_name: &str) -> Result<PairResponse> {
        let response = self
            .client
            .post(format!("{}/api/v1/pair", self.base_url))
            .json(&PairRequest {
                code: code.into(),
                device_name: device_name.into(),
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    pub async fn push(&self, device_id: &str, batch: OperationBatch) -> Result<OperationResponse> {
        let response = self
            .client
            .post(format!("{}/api/v1/operations", self.base_url))
            .bearer_auth(self.token.as_deref().unwrap_or_default())
            .header("x-rtools-device-id", device_id)
            .json(&batch)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    pub async fn pull(&self, device_id: &str, after: i64) -> Result<ChangePage> {
        let response = self
            .client
            .get(format!("{}/api/v1/changes?after={after}", self.base_url))
            .bearer_auth(self.token.as_deref().unwrap_or_default())
            .header("x-rtools-device-id", device_id)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    pub async fn snapshot(&self, device_id: &str) -> Result<rtools_sync_proto::Snapshot> {
        let response = self
            .client
            .get(format!("{}/api/v1/snapshot", self.base_url))
            .bearer_auth(self.token.as_deref().unwrap_or_default())
            .header("x-rtools-device-id", device_id)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    pub async fn devices(&self, device_id: &str) -> Result<Vec<DeviceInfo>> {
        let response = self
            .client
            .get(format!("{}/api/v1/devices", self.base_url))
            .bearer_auth(self.token.as_deref().unwrap_or_default())
            .header("x-rtools-device-id", device_id)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    pub async fn create_pairing_code(&self, device_id: &str) -> Result<PairingCodeResponse> {
        let response = self
            .client
            .post(format!("{}/api/v1/pairing-codes", self.base_url))
            .bearer_auth(self.token.as_deref().unwrap_or_default())
            .header("x-rtools-device-id", device_id)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    pub async fn revoke_device(&self, device_id: &str, target: &str) -> Result<()> {
        self.client
            .delete(format!("{}/api/v1/devices/{target}", self.base_url))
            .bearer_auth(self.token.as_deref().unwrap_or_default())
            .header("x-rtools-device-id", device_id)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

pub fn local_to_patch(task: &Task) -> rtools_domain::TaskPatch {
    rtools_domain::TaskPatch {
        title: Some(task.title.clone()),
        priority: Some(task.priority),
        remind_at: Some(task.remind_at),
        remind_offset_min: Some(task.remind_offset_min),
        repeat_rule: Some(task.repeat_rule.clone()),
        sort_order: Some(task.sort_order),
        completed_at: Some(task.completed_at),
        deleted_at: Some(task.deleted_at),
        updated_at: Some(Utc::now()),
    }
}
