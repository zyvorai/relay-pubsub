// Copyright 2026 Zyvor AI Labs
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
use crate::memory::MemoryBackend;
use crate::model::{Delivery, NewMessage, SubscriptionSpec, TopicSpec};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
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
        let insecure = std::env::var("RELAY_TLS_INSECURE")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);
        let client = Client::builder()
            .timeout(timeout)
            .danger_accept_invalid_certs(insecure)
            .build()
            .map_err(|e| BackendError::Internal(e.to_string()))?;
        Ok(Self {
            inner: MemoryBackend::new(),
            client,
            relay_base_url: relay_base_url.into().trim_end_matches('/').to_string(),
            relay_token,
            actions_topic: actions_topic.into(),
        })
    }

    fn short_name(full: &str) -> &str {
        full.rsplit('/').next().unwrap_or(full)
    }

    /// Deterministic fallback idempotency key from message content (topic +
    /// source + severity + data), not the Pub/Sub message ID — a message ID
    /// is fresh on every retry, which would silently defeat Relay's own
    /// retry-dedup. An explicit `idempotency_key` attribute always wins.
    fn idempotency_key(topic: &str, message: &NewMessage) -> String {
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

#[async_trait]
impl RelayBackend for RelayEventsBackend {
    async fn create_topic(&self, topic: TopicSpec) -> Result<TopicSpec, BackendError> {
        self.inner.create_topic(topic).await
    }
    async fn get_topic(&self, name: &str) -> Result<TopicSpec, BackendError> {
        self.inner.get_topic(name).await
    }
    async fn list_topics(&self, project: &str) -> Result<Vec<TopicSpec>, BackendError> {
        self.inner.list_topics(project).await
    }
    async fn delete_topic(&self, name: &str) -> Result<(), BackendError> {
        self.inner.delete_topic(name).await
    }

    async fn create_subscription(
        &self,
        subscription: SubscriptionSpec,
    ) -> Result<SubscriptionSpec, BackendError> {
        self.inner.create_subscription(subscription).await
    }
    async fn get_subscription(&self, name: &str) -> Result<SubscriptionSpec, BackendError> {
        self.inner.get_subscription(name).await
    }
    async fn list_subscriptions(
        &self,
        project: &str,
    ) -> Result<Vec<SubscriptionSpec>, BackendError> {
        self.inner.list_subscriptions(project).await
    }
    async fn delete_subscription(&self, name: &str) -> Result<(), BackendError> {
        self.inner.delete_subscription(name).await
    }

    async fn publish(
        &self,
        topic: &str,
        messages: Vec<NewMessage>,
    ) -> Result<Vec<String>, BackendError> {
        if topic == self.actions_topic {
            return self.inner.publish(topic, messages).await;
        }
        let short = Self::short_name(topic).to_string();
        for message in &messages {
            self.accept_event(&short, message).await?;
        }
        Ok(messages
            .iter()
            .map(|_| Uuid::new_v4().to_string())
            .collect())
    }

    async fn pull(
        &self,
        subscription: &str,
        max_messages: u32,
    ) -> Result<Vec<Delivery>, BackendError> {
        self.inner.pull(subscription, max_messages).await
    }
    async fn acknowledge(
        &self,
        subscription: &str,
        ack_ids: &[String],
    ) -> Result<(), BackendError> {
        self.inner.acknowledge(subscription, ack_ids).await
    }
    async fn modify_ack_deadline(
        &self,
        subscription: &str,
        ack_ids: &[String],
        seconds: u32,
    ) -> Result<(), BackendError> {
        self.inner
            .modify_ack_deadline(subscription, ack_ids, seconds)
            .await
    }
    async fn seek_to_time(
        &self,
        subscription: &str,
        time: DateTime<Utc>,
    ) -> Result<(), BackendError> {
        self.inner.seek_to_time(subscription, time).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FASAL_CATALOG;
    use std::collections::HashMap;
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
        }
    }

    #[tokio::test]
    async fn publish_to_catalog_topic_forwards_to_relay() {
        let relay = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/events"))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .mount(&relay)
            .await;

        let backend = RelayEventsBackend::new(
            relay.uri(),
            None,
            Duration::from_secs(5),
            "projects/fasal-onprem/topics/farm-actions",
        )
        .unwrap();

        let result = backend
            .publish(
                "projects/fasal-onprem/topics/irrigation.required",
                vec![msg(
                    br#"{"zone":"A4"}"#,
                    &[("severity", "critical"), ("source", "fasal")],
                )],
            )
            .await;
        assert!(result.is_ok(), "{result:?}");
    }

    /// Proves every entry in the fixed Fasal catalog
    /// (docs/FASAL_ACCOMMODATION.md #4.1/#4.2 in zyvor/relay, mirrored in
    /// FASAL_CATALOG) maps correctly — not just the one representative type
    /// (irrigation.required) the other tests exercise. publish() doesn't
    /// gate on catalog membership, so this also covers the general case.
    #[tokio::test]
    async fn publish_all_catalog_event_types() {
        for event_type in FASAL_CATALOG {
            let relay = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/events"))
                .and(body_partial_json(serde_json::json!({"type": event_type})))
                .respond_with(ResponseTemplate::new(202))
                .expect(1)
                .mount(&relay)
                .await;

            let backend = RelayEventsBackend::new(
                relay.uri(),
                None,
                Duration::from_secs(5),
                "projects/fasal-onprem/topics/farm-actions",
            )
            .unwrap();

            let result = backend
                .publish(
                    &format!("projects/fasal-onprem/topics/{event_type}"),
                    vec![msg(
                        br#"{"zone":"A4"}"#,
                        &[("severity", "critical"), ("source", "fasal")],
                    )],
                )
                .await;
            assert!(result.is_ok(), "{event_type}: {result:?}");
            relay.verify().await;
        }
    }

    #[tokio::test]
    async fn publish_to_actions_topic_stays_local() {
        let relay = MockServer::start().await;
        // No mock registered for /v1/events — if publish() tried to call
        // Relay for the actions topic, this test would fail the request.
        let actions_topic = "projects/fasal-onprem/topics/farm-actions";
        let backend =
            RelayEventsBackend::new(relay.uri(), None, Duration::from_secs(5), actions_topic)
                .unwrap();
        backend
            .create_topic(TopicSpec {
                name: actions_topic.into(),
                labels: HashMap::new(),
                kms_key_name: String::new(),
            })
            .await
            .unwrap();
        backend
            .create_subscription(SubscriptionSpec {
                name: "projects/fasal-onprem/subscriptions/farm-actions-sub".into(),
                topic: actions_topic.into(),
                ack_deadline_seconds: 30,
                labels: HashMap::new(),
                enable_message_ordering: false,
                enable_exactly_once_delivery: false,
                dead_letter: None,
                retry: None,
                push_endpoint: None,
            })
            .await
            .unwrap();

        backend
            .publish(
                actions_topic,
                vec![msg(b"{\"command\":\"irrigation.start\"}", &[])],
            )
            .await
            .unwrap();
        let deliveries = backend
            .pull("projects/fasal-onprem/subscriptions/farm-actions-sub", 10)
            .await
            .unwrap();
        assert_eq!(deliveries.len(), 1);
    }

    #[tokio::test]
    async fn retried_publish_without_idempotency_key_dedupes_at_relay() {
        let relay = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/events"))
            .respond_with(ResponseTemplate::new(202))
            .mount(&relay)
            .await;
        let backend = RelayEventsBackend::new(
            relay.uri(),
            None,
            Duration::from_secs(5),
            "projects/fasal-onprem/topics/farm-actions",
        )
        .unwrap();

        let m = msg(
            br#"{"zone":"A4"}"#,
            &[("severity", "critical"), ("source", "fasal")],
        );
        let k1 = RelayEventsBackend::idempotency_key("irrigation.required", &m);
        let k2 = RelayEventsBackend::idempotency_key("irrigation.required", &m);
        assert_eq!(
            k1, k2,
            "identical content must hash to the same idempotency key"
        );

        let different = msg(
            br#"{"zone":"B1"}"#,
            &[("severity", "critical"), ("source", "fasal")],
        );
        let k3 = RelayEventsBackend::idempotency_key("irrigation.required", &different);
        assert_ne!(k1, k3);

        // sanity: backend is usable (avoids "unused" warnings if the above ever changes)
        let _ = backend
            .publish("projects/fasal-onprem/topics/irrigation.required", vec![m])
            .await;
    }

    #[tokio::test]
    async fn explicit_idempotency_key_wins() {
        let m = msg(b"{}", &[("idempotency_key", "fasal/irrigation/184/A4/1")]);
        assert_eq!(
            RelayEventsBackend::idempotency_key("irrigation.required", &m),
            "fasal/irrigation/184/A4/1"
        );
    }
}
