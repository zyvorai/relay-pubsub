// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

use crate::backend::{BackendError, RelayBackend};
use crate::metrics::Metrics;
use crate::model::{DeadLetterSpec, Delivery, NewMessage, RetrySpec, SubscriptionSpec, TopicSpec};
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Clone)]
pub struct HttpState {
    pub backend: Arc<dyn RelayBackend>,
    pub auth_token: Option<String>,
    pub metrics: Metrics,
}

pub fn router(state: HttpState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(health))
        .route("/metrics", get(metrics))
        .route("/admin/v1/topics", get(admin_topics))
        .route("/admin/v1/subscriptions", get(admin_subscriptions))
        .route("/admin/v1/publish", post(admin_publish))
        .route("/admin/v1/pull", post(admin_pull))
        .route("/admin/v1/ack", post(admin_ack))
        .route("/v1/*path", any(google_dispatch))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(json!({"status":"ok","service":"relay-pubsub"}))
}

async fn metrics(State(state): State<HttpState>) -> impl IntoResponse {
    (
        [("content-type", "text/plain; version=0.0.4")],
        state.metrics.render(),
    )
}

fn authorized(state: &HttpState, headers: &HeaderMap) -> bool {
    let Some(expected) = &state.auth_token else {
        return true;
    };
    let expected_header = format!("Bearer {expected}");
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(expected_header.as_str())
}

fn backend_error(error: BackendError) -> Response {
    let (status, code) = match &error {
        BackendError::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
        BackendError::AlreadyExists(_) => (StatusCode::CONFLICT, "ALREADY_EXISTS"),
        BackendError::InvalidArgument(_) => (StatusCode::BAD_REQUEST, "INVALID_ARGUMENT"),
        BackendError::FailedPrecondition(_) => {
            (StatusCode::PRECONDITION_FAILED, "FAILED_PRECONDITION")
        }
        BackendError::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE"),
        BackendError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL"),
    };
    let body =
        Json(json!({"error":{"code":status.as_u16(),"message":error.to_string(),"status":code}}));
    (status, body).into_response()
}

fn bad_request(message: impl Into<String>) -> Response {
    let message = message.into();
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":{"code":400,"message":message,"status":"INVALID_ARGUMENT"}})),
    )
        .into_response()
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"error":{"code":401,"message":"missing or invalid bearer token","status":"UNAUTHENTICATED"}}))).into_response()
}

fn topic_json(topic: TopicSpec) -> Value {
    json!({"name": topic.name, "labels": topic.labels, "kmsKeyName": topic.kms_key_name})
}

fn subscription_json(sub: SubscriptionSpec) -> Value {
    json!({
        "name": sub.name,
        "topic": sub.topic,
        "ackDeadlineSeconds": sub.ack_deadline_seconds,
        "labels": sub.labels,
        "enableMessageOrdering": sub.enable_message_ordering,
        "enableExactlyOnceDelivery": sub.enable_exactly_once_delivery,
        "pushConfig": sub.push_endpoint.map(|endpoint| json!({"pushEndpoint": endpoint})).unwrap_or_else(|| json!({})),
        "deadLetterPolicy": sub.dead_letter.map(|d| json!({"deadLetterTopic": d.topic, "maxDeliveryAttempts": d.max_delivery_attempts})),
        "retryPolicy": sub.retry.map(|r| json!({"minimumBackoff": format!("{}s", r.minimum_backoff_seconds), "maximumBackoff": format!("{}s", r.maximum_backoff_seconds)})),
    })
}

fn delivery_json(delivery: Delivery) -> Value {
    json!({
        "ackId": delivery.ack_id,
        "deliveryAttempt": delivery.delivery_attempt,
        "message": {
            "data": BASE64.encode(&delivery.message.data),
            "attributes": delivery.message.attributes,
            "messageId": delivery.message.id,
            "publishTime": delivery.message.published_at.to_rfc3339(),
            "orderingKey": delivery.message.ordering_key,
        }
    })
}

fn parse_messages(body: &Value) -> Result<Vec<NewMessage>, Response> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| bad_request("messages array is required"))?;
    if messages.is_empty() {
        return Err(bad_request("at least one message is required"));
    }
    messages
        .iter()
        .map(|m| {
            let data = m.get("data").and_then(Value::as_str).unwrap_or("");
            let decoded = BASE64
                .decode(data)
                .map_err(|_| bad_request("message.data must be base64"))?;
            let attributes = m
                .get("attributes")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| bad_request("attributes must be string map"))?
                .unwrap_or_default();
            let ordering_key = m
                .get("orderingKey")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Ok(NewMessage {
                data: decoded,
                attributes,
                ordering_key,
            })
        })
        .collect()
}

fn parse_duration_seconds(value: Option<&Value>) -> Option<u32> {
    let raw = value?.as_str()?;
    raw.strip_suffix('s')?
        .parse::<f64>()
        .ok()
        .map(|v| v.max(0.0) as u32)
}

async fn google_dispatch(
    State(state): State<HttpState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let timer = state
        .metrics
        .latency
        .with_label_values(&["rest", "dispatch"])
        .start_timer();
    let path = uri.path().trim_start_matches("/v1/");
    let value: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return bad_request(format!("invalid json: {e}")),
        }
    };

    let response = if method == Method::POST && path.ends_with(":publish") {
        let topic = path.trim_end_matches(":publish").to_string();
        match parse_messages(&value) {
            Ok(messages) => {
                let count = messages.len();
                match state.backend.publish(&topic, messages).await {
                    Ok(ids) => {
                        state
                            .metrics
                            .messages
                            .with_label_values(&["published"])
                            .inc_by(count as u64);
                        Json(json!({"messageIds": ids})).into_response()
                    }
                    Err(e) => backend_error(e),
                }
            }
            Err(e) => e,
        }
    } else if method == Method::POST && path.ends_with(":pull") {
        let subscription = path.trim_end_matches(":pull");
        let max = value
            .get("maxMessages")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .clamp(1, 1000) as u32;
        match state.backend.pull(subscription, max).await {
            Ok(deliveries) => {
                state
                    .metrics
                    .messages
                    .with_label_values(&["delivered"])
                    .inc_by(deliveries.len() as u64);
                Json(json!({"receivedMessages": deliveries.into_iter().map(delivery_json).collect::<Vec<_>>() })).into_response()
            }
            Err(e) => backend_error(e),
        }
    } else if method == Method::POST && path.ends_with(":acknowledge") {
        let subscription = path.trim_end_matches(":acknowledge");
        let ack_ids: Vec<String> = value
            .get("ackIds")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        match state.backend.acknowledge(subscription, &ack_ids).await {
            Ok(()) => Json(json!({})).into_response(),
            Err(e) => backend_error(e),
        }
    } else if method == Method::POST && path.ends_with(":modifyAckDeadline") {
        let subscription = path.trim_end_matches(":modifyAckDeadline");
        let ack_ids: Vec<String> = value
            .get("ackIds")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let seconds = value
            .get("ackDeadlineSeconds")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if seconds > 600 {
            bad_request("ackDeadlineSeconds must be <= 600")
        } else {
            match state
                .backend
                .modify_ack_deadline(subscription, &ack_ids, seconds as u32)
                .await
            {
                Ok(()) => Json(json!({})).into_response(),
                Err(e) => backend_error(e),
            }
        }
    } else if method == Method::POST && path.ends_with(":seek") {
        let subscription = path.trim_end_matches(":seek");
        let time = value.get("time").and_then(Value::as_str);
        match time
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
        {
            Some(time) => match state.backend.seek_to_time(subscription, time).await {
                Ok(()) => Json(json!({})).into_response(),
                Err(e) => backend_error(e),
            },
            None => bad_request(
                "time must be an RFC3339 timestamp; snapshot seek is not implemented in v0.1",
            ),
        }
    } else {
        handle_resource_request(&state, method, path, value).await
    };
    timer.observe_duration();
    response
}

async fn handle_resource_request(
    state: &HttpState,
    method: Method,
    path: &str,
    value: Value,
) -> Response {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 3 || parts[0] != "projects" {
        return (StatusCode::NOT_FOUND, Json(json!({"error":{"code":404,"message":"unknown Pub/Sub REST path","status":"NOT_FOUND"}}))).into_response();
    }
    let project = format!("projects/{}", parts[1]);

    if parts.len() == 3 && parts[2] == "topics" && method == Method::GET {
        return match state.backend.list_topics(&project).await {
            Ok(topics) => {
                Json(json!({"topics": topics.into_iter().map(topic_json).collect::<Vec<_>>() }))
                    .into_response()
            }
            Err(e) => backend_error(e),
        };
    }
    if parts.len() == 3 && parts[2] == "subscriptions" && method == Method::GET {
        return match state.backend.list_subscriptions(&project).await {
            Ok(subs) => Json(json!({"subscriptions": subs.into_iter().map(subscription_json).collect::<Vec<_>>() })).into_response(),
            Err(e) => backend_error(e),
        };
    }

    if parts.len() == 4 && parts[2] == "topics" {
        let name = path.to_string();
        return match method {
            Method::PUT => {
                let labels = value
                    .get("labels")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                let kms_key_name = value
                    .get("kmsKeyName")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                match state
                    .backend
                    .create_topic(TopicSpec {
                        name,
                        labels,
                        kms_key_name,
                    })
                    .await
                {
                    Ok(topic) => (StatusCode::OK, Json(topic_json(topic))).into_response(),
                    Err(e) => backend_error(e),
                }
            }
            Method::GET => match state.backend.get_topic(&name).await {
                Ok(t) => Json(topic_json(t)).into_response(),
                Err(e) => backend_error(e),
            },
            Method::DELETE => match state.backend.delete_topic(&name).await {
                Ok(()) => Json(json!({})).into_response(),
                Err(e) => backend_error(e),
            },
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        };
    }

    if parts.len() == 4 && parts[2] == "subscriptions" {
        let name = path.to_string();
        return match method {
            Method::PUT => {
                let Some(topic) = value.get("topic").and_then(Value::as_str) else {
                    return bad_request("topic is required");
                };
                let labels = value
                    .get("labels")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                let dead_letter = value.get("deadLetterPolicy").and_then(|v| {
                    Some(DeadLetterSpec {
                        topic: v.get("deadLetterTopic")?.as_str()?.to_string(),
                        max_delivery_attempts: v
                            .get("maxDeliveryAttempts")
                            .and_then(Value::as_u64)
                            .unwrap_or(5) as u32,
                    })
                });
                let retry = value.get("retryPolicy").map(|v| RetrySpec {
                    minimum_backoff_seconds: parse_duration_seconds(v.get("minimumBackoff"))
                        .unwrap_or(0),
                    maximum_backoff_seconds: parse_duration_seconds(v.get("maximumBackoff"))
                        .unwrap_or(0),
                });
                let push_endpoint = value
                    .get("pushConfig")
                    .and_then(|v| v.get("pushEndpoint"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let spec = SubscriptionSpec {
                    name,
                    topic: topic.to_string(),
                    ack_deadline_seconds: value
                        .get("ackDeadlineSeconds")
                        .and_then(Value::as_u64)
                        .unwrap_or(10) as u32,
                    labels,
                    enable_message_ordering: value
                        .get("enableMessageOrdering")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    enable_exactly_once_delivery: value
                        .get("enableExactlyOnceDelivery")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    dead_letter,
                    retry,
                    push_endpoint,
                };
                match state.backend.create_subscription(spec).await {
                    Ok(sub) => Json(subscription_json(sub)).into_response(),
                    Err(e) => backend_error(e),
                }
            }
            Method::GET => match state.backend.get_subscription(&name).await {
                Ok(s) => Json(subscription_json(s)).into_response(),
                Err(e) => backend_error(e),
            },
            Method::DELETE => match state.backend.delete_subscription(&name).await {
                Ok(()) => Json(json!({})).into_response(),
                Err(e) => backend_error(e),
            },
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        };
    }

    (StatusCode::NOT_FOUND, Json(json!({"error":{"code":404,"message":"unknown Pub/Sub REST path","status":"NOT_FOUND"}}))).into_response()
}

#[derive(Deserialize)]
struct ProjectQuery {
    project: Option<String>,
}

async fn admin_topics(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(query): Query<ProjectQuery>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    match state
        .backend
        .list_topics(query.project.as_deref().unwrap_or("projects/demo"))
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => backend_error(e),
    }
}

async fn admin_subscriptions(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(query): Query<ProjectQuery>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    match state
        .backend
        .list_subscriptions(query.project.as_deref().unwrap_or("projects/demo"))
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => backend_error(e),
    }
}

#[derive(Deserialize)]
struct AdminPublish {
    topic: String,
    data: String,
    #[serde(default)]
    attributes: HashMap<String, String>,
    #[serde(default)]
    ordering_key: String,
}

async fn admin_publish(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(req): Json<AdminPublish>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    match state
        .backend
        .publish(
            &req.topic,
            vec![NewMessage {
                data: req.data.into_bytes(),
                attributes: req.attributes,
                ordering_key: req.ordering_key,
            }],
        )
        .await
    {
        Ok(ids) => Json(json!({"message_ids": ids})).into_response(),
        Err(e) => backend_error(e),
    }
}

#[derive(Deserialize)]
struct AdminPull {
    subscription: String,
    #[serde(default = "default_pull")]
    max_messages: u32,
}
fn default_pull() -> u32 {
    10
}

async fn admin_pull(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(req): Json<AdminPull>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    match state
        .backend
        .pull(&req.subscription, req.max_messages)
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => backend_error(e),
    }
}

#[derive(Deserialize)]
struct AdminAck {
    subscription: String,
    ack_ids: Vec<String>,
}

async fn admin_ack(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(req): Json<AdminAck>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    match state
        .backend
        .acknowledge(&req.subscription, &req.ack_ids)
        .await
    {
        Ok(()) => Json(json!({"ok":true})).into_response(),
        Err(e) => backend_error(e),
    }
}
