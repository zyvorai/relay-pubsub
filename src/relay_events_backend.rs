// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

//! RelayEventsBackend targets Zyvor Relay's real, already-shipped API
//! (event-lifecycle shaped: `POST /v1/events`) instead of the invented
//! topics/subscriptions REST contract `http_backend.rs` speaks. See
//! docs/RELAY_EVENTS_BACKEND.md.
//!
//! Topic/subscription CRUD, pull, acknowledge, modify-ack-deadline and
//! seek-to-time all delegate unchanged to an inner `MemoryBackend` — its
//! DLQ, nack/redelivery and seek logic apply for free. The only behavior
//! this backend adds is on `publish`: publishing to the designated actions
//! topic queues the message locally (matching MemoryBackend's normal
//! store-and-forward semantics, since that's the Relay -> Fasal direction);
//! publishing to any other topic is the Fasal -> Relay direction and is
//! forwarded to Relay's `POST /v1/events` instead of being stored.

use crate::backend::{BackendError, RelayBackend};
use crate::cloudevents;
use crate::memory::MemoryBackend;
use crate::model::{
    Delivery, IamPolicy, NewMessage, Page, SchemaSpec, SnapshotSpec, SubscriptionSpec, TopicSpec,
};
use crate::schema_validate;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

pub struct RelayEventsBackend {
    inner: MemoryBackend,
    client: Client,
    relay_base_url: String,
    relay_token: Option<String>,
    /// Full resource name (e.g. "projects/fasal-onprem/topics/farm-actions").
    /// Publishes to this topic store-and-forward via `inner`; everything
    /// else forwards to Relay's real API.
    pub actions_topic: String,
}

#[derive(Serialize)]
struct RelayEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    source: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    severity: String,
    idempotency_key: String,
    data: Value,
}

impl RelayEventsBackend {
    pub fn new(
        relay_base_url: impl Into<String>,
        relay_token: Option<String>,
        timeout: Duration,
        actions_topic: impl Into<String>,
    ) -> Result<Self, BackendError> {
        Self::new_inner(
            MemoryBackend::new(),
            relay_base_url,
            relay_token,
            timeout,
            actions_topic,
        )
    }

    pub fn with_persistence(
        persist_path: impl AsRef<Path>,
        relay_base_url: impl Into<String>,
        relay_token: Option<String>,
        timeout: Duration,
        actions_topic: impl Into<String>,
    ) -> Result<Self, BackendError> {
        Self::new_inner(
            MemoryBackend::with_persistence(persist_path)?,
            relay_base_url,
            relay_token,
            timeout,
            actions_topic,
        )
    }

    fn new_inner(
        inner: MemoryBackend,
        relay_base_url: impl Into<String>,
        relay_token: Option<String>,
        timeout: Duration,
        actions_topic: impl Into<String>,
    ) -> Result<Self, BackendError> {
        let insecure = std::env::var("RELAY_TLS_INSECURE")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);
        let client = Client::builder()
            .timeout(timeout)
            .danger_accept_invalid_certs(insecure)
            .build()
            .map_err(|e| BackendError::Internal(e.to_string()))?;
        Ok(Self {
            inner,
            client,
            relay_base_url: relay_base_url.into().trim_end_matches('/').to_string(),
            relay_token,
            actions_topic: actions_topic.into(),
        })
    }

    /// Probe Relay reachability for readiness checks.
    pub async fn relay_reachable(&self) -> bool {
        let url = format!("{}/healthz", self.relay_base_url);
        let mut req = self.client.get(&url);
        if let Some(token) = &self.relay_token {
            if token != "demo-token" {
                req = req.bearer_auth(token);
            }
        }
        match req.send().await {
            Ok(resp) => resp.status().is_success() || resp.status().as_u16() == 404,
            Err(_) => {
                // Fall back: POST path existence isn't required; try base URL.
                self.client
                    .get(&self.relay_base_url)
                    .send()
                    .await
                    .map(|r| r.status().as_u16() < 500)
                    .unwrap_or(false)
            }
        }
    }

    fn short_name(full: &str) -> &str {
        full.rsplit('/').next().unwrap_or(full)
    }

    async fn prepare_publish(
        &self,
        topic: &str,
        messages: &mut [NewMessage],
    ) -> Result<(), BackendError> {
        let spec = self.inner.get_topic(topic).await.ok();
        let schema = if let Some(spec) = &spec {
            if spec.schema_name.is_empty() {
                None
            } else {
                Some((
                    spec.schema_encoding.clone(),
                    self.inner.get_schema(&spec.schema_name).await?,
                ))
            }
        } else {
            None
        };
        for message in messages.iter_mut() {
            cloudevents::enrich_message(topic, message);
            if let Some((encoding, schema)) = &schema {
                schema_validate::validate_payload(schema, encoding, &message.data)?;
            }
        }
        Ok(())
    }

    fn idempotency_key(topic: &str, message: &NewMessage) -> String {
        if !message.message_id.is_empty() {
            return message.message_id.clone();
        }
        if let Some(key) = message.attributes.get("idempotency_key") {
            if !key.is_empty() {
                return key.clone();
            }
        }
        let mut hasher = Sha256::new();
        hasher.update(topic.as_bytes());
        hasher.update(b"|");
        hasher.update(
            message
                .attributes
                .get("source")
                .map(String::as_str)
                .unwrap_or("")
                .as_bytes(),
        );
        hasher.update(b"|");
        hasher.update(
            message
                .attributes
                .get("severity")
                .map(String::as_str)
                .unwrap_or("")
                .as_bytes(),
        );
        hasher.update(b"|");
        hasher.update(&message.data);
        let digest = hasher.finalize();
        let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        format!("auto/{hex}")
    }

    fn data_or_raw(data: &[u8]) -> Value {
        if data.is_empty() {
            return Value::Object(Map::new());
        }
        serde_json::from_slice(data).unwrap_or_else(|_| json!({"raw": base64_encode(data)}))
    }

    async fn accept_event(
        &self,
        topic_short: &str,
        message: &NewMessage,
    ) -> Result<(), BackendError> {
        let event = RelayEvent {
            event_type: topic_short.to_string(),
            source: message
                .attributes
                .get("source")
                .cloned()
                .unwrap_or_default(),
            severity: message
                .attributes
                .get("severity")
                .cloned()
                .unwrap_or_default(),
            idempotency_key: Self::idempotency_key(topic_short, message),
            data: Self::data_or_raw(&message.data),
        };

        let url = match &self.relay_token {
            Some(t) if t == "demo-token" => {
                format!("{}/v1/events?token=demo-token", self.relay_base_url)
            }
            _ => format!("{}/v1/events", self.relay_base_url),
        };
        let mut req = self.client.post(url).json(&event);
        if let Some(token) = &self.relay_token {
            if token != "demo-token" {
                req = req.bearer_auth(token);
            }
        }
        let resp = req
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        if resp.status().is_success() {
            return Ok(());
        }
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(BackendError::Internal(format!("relay {status}: {text}")))
    }
}

fn base64_encode(data: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.encode(data)
}

macro_rules! delegate {
    ($self:ident . $method:ident ( $($arg:expr),* $(,)? )) => {
        $self.inner.$method($($arg),*).await
    };
}

#[async_trait]
impl RelayBackend for RelayEventsBackend {
    async fn create_topic(&self, topic: TopicSpec) -> Result<TopicSpec, BackendError> {
        delegate!(self.create_topic(topic))
    }
    async fn update_topic(
        &self,
        topic: TopicSpec,
        update_mask: &[String],
    ) -> Result<TopicSpec, BackendError> {
        delegate!(self.update_topic(topic, update_mask))
    }
    async fn get_topic(&self, name: &str) -> Result<TopicSpec, BackendError> {
        delegate!(self.get_topic(name))
    }
    async fn list_topics(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<TopicSpec>, BackendError> {
        delegate!(self.list_topics(project, page_size, page_token))
    }
    async fn list_topic_subscriptions(
        &self,
        topic: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<String>, BackendError> {
        delegate!(self.list_topic_subscriptions(topic, page_size, page_token))
    }
    async fn delete_topic(&self, name: &str) -> Result<(), BackendError> {
        delegate!(self.delete_topic(name))
    }

    async fn create_subscription(
        &self,
        subscription: SubscriptionSpec,
    ) -> Result<SubscriptionSpec, BackendError> {
        delegate!(self.create_subscription(subscription))
    }
    async fn update_subscription(
        &self,
        subscription: SubscriptionSpec,
        update_mask: &[String],
    ) -> Result<SubscriptionSpec, BackendError> {
        delegate!(self.update_subscription(subscription, update_mask))
    }
    async fn get_subscription(&self, name: &str) -> Result<SubscriptionSpec, BackendError> {
        delegate!(self.get_subscription(name))
    }
    async fn list_subscriptions(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SubscriptionSpec>, BackendError> {
        delegate!(self.list_subscriptions(project, page_size, page_token))
    }
    async fn delete_subscription(&self, name: &str) -> Result<(), BackendError> {
        delegate!(self.delete_subscription(name))
    }
    async fn modify_push_config(
        &self,
        subscription: &str,
        push_endpoint: Option<String>,
        push_attributes: HashMap<String, String>,
    ) -> Result<(), BackendError> {
        delegate!(self.modify_push_config(subscription, push_endpoint, push_attributes))
    }

    async fn publish(
        &self,
        topic: &str,
        mut messages: Vec<NewMessage>,
    ) -> Result<Vec<String>, BackendError> {
        self.prepare_publish(topic, &mut messages).await?;
        if topic == self.actions_topic {
            return self.inner.publish(topic, messages).await;
        }
        let short = Self::short_name(topic).to_string();
        let mut ids = Vec::with_capacity(messages.len());
        for message in &messages {
            self.accept_event(&short, message).await?;
            ids.push(if message.message_id.is_empty() {
                Uuid::new_v4().to_string()
            } else {
                message.message_id.clone()
            });
        }
        Ok(ids)
    }

    async fn pull(
        &self,
        subscription: &str,
        max_messages: u32,
    ) -> Result<Vec<Delivery>, BackendError> {
        delegate!(self.pull(subscription, max_messages))
    }
    async fn acknowledge(
        &self,
        subscription: &str,
        ack_ids: &[String],
    ) -> Result<(), BackendError> {
        delegate!(self.acknowledge(subscription, ack_ids))
    }
    async fn modify_ack_deadline(
        &self,
        subscription: &str,
        ack_ids: &[String],
        seconds: u32,
    ) -> Result<(), BackendError> {
        delegate!(self.modify_ack_deadline(subscription, ack_ids, seconds))
    }
    async fn seek_to_time(
        &self,
        subscription: &str,
        time: DateTime<Utc>,
    ) -> Result<(), BackendError> {
        delegate!(self.seek_to_time(subscription, time))
    }
    async fn seek_to_snapshot(
        &self,
        subscription: &str,
        snapshot: &str,
    ) -> Result<(), BackendError> {
        delegate!(self.seek_to_snapshot(subscription, snapshot))
    }

    async fn create_snapshot(
        &self,
        name: &str,
        subscription: &str,
        labels: HashMap<String, String>,
    ) -> Result<SnapshotSpec, BackendError> {
        delegate!(self.create_snapshot(name, subscription, labels))
    }
    async fn update_snapshot(
        &self,
        snapshot: SnapshotSpec,
        update_mask: &[String],
    ) -> Result<SnapshotSpec, BackendError> {
        delegate!(self.update_snapshot(snapshot, update_mask))
    }
    async fn get_snapshot(&self, name: &str) -> Result<SnapshotSpec, BackendError> {
        delegate!(self.get_snapshot(name))
    }
    async fn list_snapshots(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SnapshotSpec>, BackendError> {
        delegate!(self.list_snapshots(project, page_size, page_token))
    }
    async fn delete_snapshot(&self, name: &str) -> Result<(), BackendError> {
        delegate!(self.delete_snapshot(name))
    }

    async fn get_iam_policy(&self, resource: &str) -> Result<IamPolicy, BackendError> {
        delegate!(self.get_iam_policy(resource))
    }
    async fn set_iam_policy(
        &self,
        resource: &str,
        policy: IamPolicy,
    ) -> Result<IamPolicy, BackendError> {
        delegate!(self.set_iam_policy(resource, policy))
    }
    async fn test_iam_permissions(
        &self,
        resource: &str,
        permissions: &[String],
    ) -> Result<Vec<String>, BackendError> {
        delegate!(self.test_iam_permissions(resource, permissions))
    }

    async fn create_schema(&self, schema: SchemaSpec) -> Result<SchemaSpec, BackendError> {
        delegate!(self.create_schema(schema))
    }
    async fn get_schema(&self, name: &str) -> Result<SchemaSpec, BackendError> {
        delegate!(self.get_schema(name))
    }
    async fn list_schemas(
        &self,
        parent: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SchemaSpec>, BackendError> {
        delegate!(self.list_schemas(parent, page_size, page_token))
    }
    async fn delete_schema(&self, name: &str) -> Result<(), BackendError> {
        delegate!(self.delete_schema(name))
    }
    async fn validate_schema(&self, schema: &SchemaSpec) -> Result<(), BackendError> {
        delegate!(self.validate_schema(schema))
    }
    async fn validate_message(
        &self,
        schema_name: Option<&str>,
        schema: Option<&SchemaSpec>,
        message: &[u8],
    ) -> Result<(), BackendError> {
        delegate!(self.validate_message(schema_name, schema, message))
    }

    async fn list_push_subscriptions(&self) -> Result<Vec<SubscriptionSpec>, BackendError> {
        delegate!(self.list_push_subscriptions())
    }

    async fn inventory(&self, project: &str) -> Result<crate::model::InventoryReport, BackendError> {
        delegate!(self.inventory(project))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FASAL_CATALOG;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn msg(data: &[u8], attrs: &[(&str, &str)]) -> NewMessage {
        NewMessage {
            data: data.to_vec(),
            attributes: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ordering_key: String::new(),
        message_id: String::new(),
        }
    }

    #[tokio::test]
    async fn publish_to_catalog_topic_forwards_to_relay() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/events"))
            .and(body_partial_json(json!({"type": "irrigation.required"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&server)
            .await;

        let backend = RelayEventsBackend::new(
            server.uri(),
            Some("tok".into()),
            Duration::from_secs(5),
            "projects/fasal-onprem/topics/farm-actions",
        )
        .unwrap();
        let topic = format!("projects/fasal-onprem/topics/{}", FASAL_CATALOG[0]);
        backend
            .create_topic(TopicSpec {
                name: topic.clone(),
                labels: HashMap::new(),
                kms_key_name: String::new(),
            schema_name: String::new(),
            schema_encoding: String::new(),
            })
            .await
            .unwrap();
        let ids = backend
            .publish(
                &topic,
                vec![msg(
                    br#"{"field":"A"}"#,
                    &[("source", "fasal"), ("severity", "critical")],
                )],
            )
            .await
            .unwrap();
        assert_eq!(ids.len(), 1);
    }

    #[tokio::test]
    async fn publish_all_catalog_event_types() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&server)
            .await;

        let backend = RelayEventsBackend::new(
            server.uri(),
            None,
            Duration::from_secs(5),
            "projects/fasal-onprem/topics/farm-actions",
        )
        .unwrap();
        for name in FASAL_CATALOG {
            let topic = format!("projects/fasal-onprem/topics/{name}");
            let _ = backend
                .create_topic(TopicSpec {
                    name: topic.clone(),
                    labels: HashMap::new(),
                    kms_key_name: String::new(),
                schema_name: String::new(),
                schema_encoding: String::new(),
                })
                .await;
            backend
                .publish(&topic, vec![msg(br#"{"ok":true}"#, &[("source", "test")])])
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn publish_to_actions_topic_stays_local() {
        let server = MockServer::start().await;
        let actions = "projects/fasal-onprem/topics/farm-actions".to_string();
        let backend =
            RelayEventsBackend::new(server.uri(), None, Duration::from_secs(5), actions.clone())
                .unwrap();
        backend
            .create_topic(TopicSpec {
                name: actions.clone(),
                labels: HashMap::new(),
                kms_key_name: String::new(),
            schema_name: String::new(),
            schema_encoding: String::new(),
            })
            .await
            .unwrap();
        backend
            .create_subscription(SubscriptionSpec {
                name: "projects/fasal-onprem/subscriptions/farm-actions-sub".into(),
                topic: actions.clone(),
                ack_deadline_seconds: 30,
                labels: HashMap::new(),
                enable_message_ordering: false,
                enable_exactly_once_delivery: false,
                dead_letter: None,
                retry: None,
                push_endpoint: None,
                push_attributes: HashMap::new(),
            filter: String::new(),
            })
            .await
            .unwrap();
        backend
            .publish(&actions, vec![msg(b"act", &[("source", "relay")])])
            .await
            .unwrap();
        let pulled = backend
            .pull("projects/fasal-onprem/subscriptions/farm-actions-sub", 10)
            .await
            .unwrap();
        assert_eq!(pulled.len(), 1);
    }

    #[tokio::test]
    async fn retried_publish_without_idempotency_key_dedupes_at_relay() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&server)
            .await;

        let backend = RelayEventsBackend::new(
            server.uri(),
            None,
            Duration::from_secs(5),
            "projects/fasal-onprem/topics/farm-actions",
        )
        .unwrap();
        let topic = "projects/fasal-onprem/topics/irrigation.required";
        let _ = backend
            .create_topic(TopicSpec {
                name: topic.into(),
                labels: HashMap::new(),
                kms_key_name: String::new(),
            schema_name: String::new(),
            schema_encoding: String::new(),
            })
            .await;
        let m = msg(br#"{"x":1}"#, &[("source", "s"), ("severity", "info")]);
        // Two publishes with same content produce same auto idempotency key —
        // Relay would dedupe; we still POST twice from gateway (Relay owns dedupe).
        // This test just ensures publish succeeds twice.
        backend.publish(topic, vec![m.clone()]).await.unwrap();
        backend.publish(topic, vec![m]).await.unwrap();
    }

    #[tokio::test]
    async fn explicit_idempotency_key_wins() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/events"))
            .and(body_partial_json(json!({"idempotency_key": "fixed-key"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&server)
            .await;

        let backend = RelayEventsBackend::new(
            server.uri(),
            None,
            Duration::from_secs(5),
            "projects/fasal-onprem/topics/farm-actions",
        )
        .unwrap();
        let topic = "projects/fasal-onprem/topics/irrigation.required";
        let _ = backend
            .create_topic(TopicSpec {
                name: topic.into(),
                labels: HashMap::new(),
                kms_key_name: String::new(),
            schema_name: String::new(),
            schema_encoding: String::new(),
            })
            .await;
        backend
            .publish(
                topic,
                vec![msg(
                    br#"{}"#,
                    &[("idempotency_key", "fixed-key"), ("source", "s")],
                )],
            )
            .await
            .unwrap();
    }
}
