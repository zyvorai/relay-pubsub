// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

//! Native gRPC Relay backend scaffold.
//!
//! Selected with `--backend grpc` / `RELAY_BACKEND=grpc`. There is no Relay
//! gRPC `.proto` in this repository yet, so every `RelayBackend` data-plane
//! method returns a clear "not fully implemented" error. When a Relay gRPC
//! API ships, replace the stub bodies without changing the Pub/Sub surface.
//!
//! Optional: `probe_http_health` GETs `{RELAY_BASE_URL}/healthz` so readiness
//! tooling can still reach Relay over HTTP while the gRPC path is unfinished.

use crate::backend::{BackendError, RelayBackend};
use crate::model::{
    Delivery, IamPolicy, NewMessage, Page, SchemaSpec, SnapshotSpec, SubscriptionSpec, TopicSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;

const NOT_IMPLEMENTED: &str = "native gRPC Relay backend is not fully implemented \
    (no Relay gRPC proto in-repo); use --backend relay-events or http";

/// Scaffold for a future tonic client against Zyvor Relay's native gRPC API.
#[derive(Clone)]
pub struct GrpcRelayBackend {
    base_url: String,
    token: Option<String>,
    client: Client,
}

impl GrpcRelayBackend {
    pub fn new(
        base_url: impl Into<String>,
        token: Option<String>,
        timeout: Duration,
    ) -> Result<Self, BackendError> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| BackendError::Internal(e.to_string()))?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            client,
        })
    }

    fn stub<T>() -> Result<T, BackendError> {
        Err(BackendError::FailedPrecondition(NOT_IMPLEMENTED.to_string()))
    }

    /// Best-effort HTTP `/healthz` probe until native Relay gRPC health exists.
    pub async fn probe_http_health(&self) -> Result<(), BackendError> {
        let url = format!("{}/healthz", self.base_url);
        let mut req = self.client.get(&url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        match req.send().await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 404 => Ok(()),
            Ok(resp) => Err(BackendError::Unavailable(format!(
                "Relay HTTP health returned {}",
                resp.status()
            ))),
            Err(e) => Err(BackendError::Unavailable(e.to_string())),
        }
    }
}

#[async_trait]
impl RelayBackend for GrpcRelayBackend {
    async fn create_topic(&self, _topic: TopicSpec) -> Result<TopicSpec, BackendError> {
        Self::stub()
    }
    async fn update_topic(
        &self,
        _topic: TopicSpec,
        _update_mask: &[String],
    ) -> Result<TopicSpec, BackendError> {
        Self::stub()
    }
    async fn get_topic(&self, _name: &str) -> Result<TopicSpec, BackendError> {
        Self::stub()
    }
    async fn list_topics(
        &self,
        _project: &str,
        _page_size: i32,
        _page_token: &str,
    ) -> Result<Page<TopicSpec>, BackendError> {
        Self::stub()
    }
    async fn list_topic_subscriptions(
        &self,
        _topic: &str,
        _page_size: i32,
        _page_token: &str,
    ) -> Result<Page<String>, BackendError> {
        Self::stub()
    }
    async fn delete_topic(&self, _name: &str) -> Result<(), BackendError> {
        Self::stub()
    }

    async fn create_subscription(
        &self,
        _subscription: SubscriptionSpec,
    ) -> Result<SubscriptionSpec, BackendError> {
        Self::stub()
    }
    async fn update_subscription(
        &self,
        _subscription: SubscriptionSpec,
        _update_mask: &[String],
    ) -> Result<SubscriptionSpec, BackendError> {
        Self::stub()
    }
    async fn get_subscription(&self, _name: &str) -> Result<SubscriptionSpec, BackendError> {
        Self::stub()
    }
    async fn list_subscriptions(
        &self,
        _project: &str,
        _page_size: i32,
        _page_token: &str,
    ) -> Result<Page<SubscriptionSpec>, BackendError> {
        Self::stub()
    }
    async fn delete_subscription(&self, _name: &str) -> Result<(), BackendError> {
        Self::stub()
    }
    async fn modify_push_config(
        &self,
        _subscription: &str,
        _push_endpoint: Option<String>,
        _push_attributes: HashMap<String, String>,
    ) -> Result<(), BackendError> {
        Self::stub()
    }

    async fn publish(
        &self,
        _topic: &str,
        _messages: Vec<NewMessage>,
    ) -> Result<Vec<String>, BackendError> {
        Self::stub()
    }
    async fn pull(
        &self,
        _subscription: &str,
        _max_messages: u32,
    ) -> Result<Vec<Delivery>, BackendError> {
        Self::stub()
    }
    async fn acknowledge(
        &self,
        _subscription: &str,
        _ack_ids: &[String],
    ) -> Result<(), BackendError> {
        Self::stub()
    }
    async fn modify_ack_deadline(
        &self,
        _subscription: &str,
        _ack_ids: &[String],
        _seconds: u32,
    ) -> Result<(), BackendError> {
        Self::stub()
    }
    async fn seek_to_time(
        &self,
        _subscription: &str,
        _time: DateTime<Utc>,
    ) -> Result<(), BackendError> {
        Self::stub()
    }
    async fn seek_to_snapshot(
        &self,
        _subscription: &str,
        _snapshot: &str,
    ) -> Result<(), BackendError> {
        Self::stub()
    }

    async fn create_snapshot(
        &self,
        _name: &str,
        _subscription: &str,
        _labels: HashMap<String, String>,
    ) -> Result<SnapshotSpec, BackendError> {
        Self::stub()
    }
    async fn update_snapshot(
        &self,
        _snapshot: SnapshotSpec,
        _update_mask: &[String],
    ) -> Result<SnapshotSpec, BackendError> {
        Self::stub()
    }
    async fn get_snapshot(&self, _name: &str) -> Result<SnapshotSpec, BackendError> {
        Self::stub()
    }
    async fn list_snapshots(
        &self,
        _project: &str,
        _page_size: i32,
        _page_token: &str,
    ) -> Result<Page<SnapshotSpec>, BackendError> {
        Self::stub()
    }
    async fn delete_snapshot(&self, _name: &str) -> Result<(), BackendError> {
        Self::stub()
    }

    async fn get_iam_policy(&self, _resource: &str) -> Result<IamPolicy, BackendError> {
        Self::stub()
    }
    async fn set_iam_policy(
        &self,
        _resource: &str,
        _policy: IamPolicy,
    ) -> Result<IamPolicy, BackendError> {
        Self::stub()
    }
    async fn test_iam_permissions(
        &self,
        _resource: &str,
        _permissions: &[String],
    ) -> Result<Vec<String>, BackendError> {
        Self::stub()
    }

    async fn create_schema(&self, _schema: SchemaSpec) -> Result<SchemaSpec, BackendError> {
        Self::stub()
    }
    async fn get_schema(&self, _name: &str) -> Result<SchemaSpec, BackendError> {
        Self::stub()
    }
    async fn list_schemas(
        &self,
        _parent: &str,
        _page_size: i32,
        _page_token: &str,
    ) -> Result<Page<SchemaSpec>, BackendError> {
        Self::stub()
    }
    async fn delete_schema(&self, _name: &str) -> Result<(), BackendError> {
        Self::stub()
    }
    async fn validate_schema(&self, _schema: &SchemaSpec) -> Result<(), BackendError> {
        Self::stub()
    }
    async fn validate_message(
        &self,
        _schema_name: Option<&str>,
        _schema: Option<&SchemaSpec>,
        _message: &[u8],
    ) -> Result<(), BackendError> {
        Self::stub()
    }

    async fn list_push_subscriptions(&self) -> Result<Vec<SubscriptionSpec>, BackendError> {
        Self::stub()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NewMessage;
    use std::time::Duration;

    #[tokio::test]
    async fn stub_returns_not_implemented() {
        let backend = GrpcRelayBackend::new("http://127.0.0.1:9", None, Duration::from_secs(1))
            .expect("construct");
        let err = backend
            .publish("projects/demo/topics/t", vec![NewMessage::default()])
            .await
            .expect_err("stub");
        let msg = err.to_string();
        assert!(
            msg.contains("not fully implemented"),
            "unexpected error: {msg}"
        );
    }
}
