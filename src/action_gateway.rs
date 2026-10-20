// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

//! Implements Zyvor Relay's Action Gateway HTTP contract
//! (`POST /v1/actions` + required `Idempotency-Key`): Relay calls this to
//! deliver a command, which is queued as a Pub/Sub message on the actions
//! topic (via the backend's normal `publish`, store-and-forward) for Fasal's
//! consumer to pull/ack. A 2xx here means "durably accepted," not
//! "physically executed" — matching the contract's own model that
//! verification is a separate, later step.

use crate::backend::RelayBackend;
use crate::model::NewMessage;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ActionGatewayState {
    pub backend: Arc<dyn RelayBackend>,
    /// Full resource name of the actions topic messages are enqueued onto.
    pub actions_topic: String,
    by_key: Arc<Mutex<HashMap<String, ActionResult>>>,
}

impl ActionGatewayState {
    pub fn new(backend: Arc<dyn RelayBackend>, actions_topic: impl Into<String>) -> Self {
        Self {
            backend,
            actions_topic: actions_topic.into(),
            by_key: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub fn router(state: ActionGatewayState) -> Router {
    Router::new()
        .route("/v1/actions", post(handle_action))
        .with_state(state)
}

#[derive(Deserialize, Default)]
struct ActionIn {
    #[serde(default)]
    action_id: String,
    #[serde(default)]
    event_id: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    payload: Value,
}

#[derive(Clone, Serialize)]
struct ActionResult {
    provider_id: String,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence: Option<Value>,
    at: DateTime<Utc>,
}

async fn handle_action(
    State(state): State<ActionGatewayState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Idempotency-Key required"})),
        )
            .into_response();
    };
    let key = key.to_string();
    let input: ActionIn = if body.is_empty() {
        ActionIn::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("invalid body: {e}")})),
                )
                    .into_response()
            }
        }
    };

    let data = json!({"command": input.command, "payload": input.payload, "action_id": input.action_id, "event_id": input.event_id});
    let attributes = HashMap::from([
        ("idempotency_key".to_string(), key.clone()),
        ("action_id".to_string(), input.action_id.clone()),
        ("event_id".to_string(), input.event_id.clone()),
        ("command".to_string(), input.command.clone()),
    ]);
    let message = NewMessage {
        data: data.to_string().into_bytes(),
        attributes,
        ordering_key: String::new(),
        message_id: String::new(),
    };

    // Hold the lock across check + publish + insert so a retried (or
    // concurrent) Idempotency-Key never enqueues a second command.
    let mut map = state.by_key.lock().await;
    if let Some(existing) = map.get(&key).cloned() {
        drop(map);
        return result_response(existing, true);
    }

    if let Err(e) = state
        .backend
        .publish(&state.actions_topic, vec![message])
        .await
    {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": e.to_string()})),
        )
            .into_response();
    }

    let candidate = ActionResult {
        provider_id: format!("rpg_{key}"),
        state: "queued".to_string(),
        evidence: Some(
            json!({"adapter": "relay-pubsub", "action_id": input.action_id, "event_id": input.event_id}),
        ),
        at: Utc::now(),
    };

    map.insert(key, candidate.clone());
    drop(map);
    result_response(candidate, false)
}

fn result_response(result: ActionResult, is_replay: bool) -> Response {
    let mut response = Json(result.clone()).into_response();
    if let Ok(v) = result.provider_id.parse() {
        response.headers_mut().insert("X-Action-ID", v);
    }
    if is_replay {
        response
            .headers_mut()
            .insert("X-Idempotent-Replay", "true".parse().unwrap());
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryBackend;
    use crate::model::{SubscriptionSpec, TopicSpec};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn make_router() -> (Router, String) {
        let actions_topic = "projects/fasal-onprem/topics/farm-actions".to_string();
        let backend: Arc<dyn RelayBackend> = Arc::new(MemoryBackend::new());
        backend
            .create_topic(TopicSpec {
                name: actions_topic.clone(),
                ..Default::default()
            })
            .await
            .unwrap();
        backend
            .create_subscription(SubscriptionSpec {
                name: "projects/fasal-onprem/subscriptions/farm-actions-sub".into(),
                topic: actions_topic.clone(),
                ack_deadline_seconds: 30,
                ..Default::default()
            })
            .await
            .unwrap();
        (
            router(ActionGatewayState::new(backend, actions_topic.clone())),
            actions_topic,
        )
    }

    #[tokio::test]
    async fn retry_does_not_enqueue_duplicate() {
        let actions_topic = "projects/fasal-onprem/topics/farm-actions".to_string();
        let sub = "projects/fasal-onprem/subscriptions/farm-actions-sub";
        let backend: Arc<dyn RelayBackend> = Arc::new(MemoryBackend::new());
        backend
            .create_topic(TopicSpec {
                name: actions_topic.clone(),
                ..Default::default()
            })
            .await
            .unwrap();
        backend
            .create_subscription(SubscriptionSpec {
                name: sub.into(),
                topic: actions_topic.clone(),
                ack_deadline_seconds: 30,
                ..Default::default()
            })
            .await
            .unwrap();
        let app = router(ActionGatewayState::new(backend.clone(), actions_topic));
        for _ in 0..3 {
            let req = Request::post("/v1/actions")
                .header("idempotency-key", "k")
                .body(Body::from(r#"{"command":"pump.start"}"#))
                .unwrap();
            assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);
        }
        let pulled = backend.pull(sub, 10).await.unwrap();
        assert_eq!(pulled.len(), 1);
    }

    #[tokio::test]
    async fn missing_idempotency_key_is_rejected() {
        let (app, _) = make_router().await;
        let req = Request::post("/v1/actions")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn idempotent_retry_returns_same_result() {
        let (app, _) = make_router().await;
        let body = r#"{"action_id":"act_1","event_id":"evt_1","command":"irrigation.start","payload":{"zone":"A4"}}"#;

        let req1 = Request::post("/v1/actions")
            .header("content-type", "application/json")
            .header("idempotency-key", "key-1")
            .body(Body::from(body))
            .unwrap();
        let resp1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        let id1 = resp1.headers().get("X-Action-ID").unwrap().clone();

        let req2 = Request::post("/v1/actions")
            .header("content-type", "application/json")
            .header("idempotency-key", "key-1")
            .body(Body::from(body))
            .unwrap();
        let resp2 = app.oneshot(req2).await.unwrap();
        assert_eq!(resp2.headers().get("X-Idempotent-Replay").unwrap(), "true");
        assert_eq!(resp2.headers().get("X-Action-ID").unwrap(), &id1);
    }
}
