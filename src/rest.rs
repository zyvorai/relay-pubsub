// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

use crate::auth::Authenticator;
use crate::backend::{BackendError, RelayBackend};
use crate::log_buffer::LogBuffer;
use crate::metrics::Metrics;
use crate::model::{
    DeadLetterSpec, Delivery, IamBinding, IamPolicy, NewMessage, RetrySpec, SchemaSpec,
    SnapshotSpec, SubscriptionSpec, TopicSpec,
};
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
use tracing::info;

#[derive(Clone)]
pub struct HttpState {
    pub backend: Arc<dyn RelayBackend>,
    pub authenticator: Option<Authenticator>,
    pub metrics: Metrics,
    /// When set, `/readyz` probes Relay reachability via `{relay_base_url}/healthz`.
    pub check_relay_ready: bool,
    pub relay_base_url: String,
    pub relay_token: Option<String>,
    pub logs: LogBuffer,
}

pub fn router(state: HttpState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/admin/v1/topics", get(admin_topics))
        .route("/admin/v1/subscriptions", get(admin_subscriptions))
        .route("/admin/v1/inventory", get(admin_inventory))
        .route("/admin/v1/publish", post(admin_publish))
        .route("/admin/v1/pull", post(admin_pull))
        .route("/admin/v1/ack", post(admin_ack))
        .route("/admin/v1/push-config", post(admin_push_config))
        .route("/admin/v1/logs", get(admin_logs))
        .route("/v1/*path", any(google_dispatch))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(json!({"status":"ok","service":"relay-pubsub"}))
}

async fn readyz(State(state): State<HttpState>) -> Response {
    if !state.check_relay_ready {
        return Json(json!({"status":"ok","service":"relay-pubsub"})).into_response();
    }
    let url = format!("{}/healthz", state.relay_base_url.trim_end_matches('/'));
    let mut req = reqwest::Client::new().get(&url);
    if let Some(token) = &state.relay_token {
        req = req.bearer_auth(token);
    }
    match req.send().await {
        Ok(resp) if resp.status().is_success() || resp.status() == StatusCode::NOT_FOUND => {
            Json(json!({"status":"ok","service":"relay-pubsub"})).into_response()
        }
        Ok(resp) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status":"unavailable",
                "service":"relay-pubsub",
                "relayStatus": resp.status().as_u16(),
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status":"unavailable",
                "service":"relay-pubsub",
                "message": err.to_string(),
            })),
        )
            .into_response(),
    }
}

async fn metrics(State(state): State<HttpState>) -> impl IntoResponse {
    (
        [("content-type", "text/plain; version=0.0.4")],
        state.metrics.render(),
    )
}

async fn authorize(state: &HttpState, headers: &HeaderMap, resource: &str) -> Result<(), Response> {
    let Some(auth) = &state.authenticator else {
        return Ok(());
    };
    let header = headers.get("authorization").and_then(|v| v.to_str().ok());
    let ctx = auth.authenticate_bearer(header).await.map_err(|message| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":{"code":401,"message":message,"status":"UNAUTHENTICATED"}})),
        )
            .into_response()
    })?;
    auth.authorize_resource(&ctx, resource).map_err(|message| {
        (
            StatusCode::FORBIDDEN,
            Json(json!({"error":{"code":403,"message":message,"status":"PERMISSION_DENIED"}})),
        )
            .into_response()
    })?;
    Ok(())
}

fn backend_error(error: BackendError) -> Response {
    let (status, code) = match &error {
        BackendError::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
        BackendError::AlreadyExists(_) => (StatusCode::CONFLICT, "ALREADY_EXISTS"),
        BackendError::InvalidArgument(_) => (StatusCode::BAD_REQUEST, "INVALID_ARGUMENT"),
        BackendError::FailedPrecondition(_) => {
            (StatusCode::PRECONDITION_FAILED, "FAILED_PRECONDITION")
        }
        BackendError::PermissionDenied(_) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED"),
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

fn not_found(message: impl Into<String>) -> Response {
    let message = message.into();
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error":{"code":404,"message":message,"status":"NOT_FOUND"}})),
    )
        .into_response()
}

fn topic_json(topic: TopicSpec) -> Value {
    json!({
        "name": topic.name,
        "labels": topic.labels,
        "kmsKeyName": topic.kms_key_name,
        "schemaSettings": if topic.schema_name.is_empty() {
            Value::Null
        } else {
            json!({"schema": topic.schema_name, "encoding": topic.schema_encoding})
        }
    })
}

fn subscription_json(sub: SubscriptionSpec) -> Value {
    json!({
        "name": sub.name,
        "topic": sub.topic,
        "ackDeadlineSeconds": sub.ack_deadline_seconds,
        "labels": sub.labels,
        "enableMessageOrdering": sub.enable_message_ordering,
        "enableExactlyOnceDelivery": sub.enable_exactly_once_delivery,
        "pushConfig": {
            "pushEndpoint": sub.push_endpoint.clone().unwrap_or_default(),
            "attributes": sub.push_attributes,
        },
        "deadLetterPolicy": sub.dead_letter.map(|d| json!({"deadLetterTopic": d.topic, "maxDeliveryAttempts": d.max_delivery_attempts})),
        "retryPolicy": sub.retry.map(|r| json!({"minimumBackoff": format!("{}s", r.minimum_backoff_seconds), "maximumBackoff": format!("{}s", r.maximum_backoff_seconds)})),
        "filter": sub.filter,
    })
}

fn snapshot_json(snapshot: SnapshotSpec) -> Value {
    json!({
        "name": snapshot.name,
        "topic": snapshot.topic,
        "expireTime": snapshot.expire_time.to_rfc3339(),
        "labels": snapshot.labels,
    })
}

fn schema_json(schema: SchemaSpec) -> Value {
    json!({
        "name": schema.name,
        "type": schema.schema_type,
        "definition": schema.definition,
    })
}

fn policy_json(policy: IamPolicy) -> Value {
    json!({
        "version": policy.version,
        "bindings": policy.bindings.into_iter().map(|b| json!({"role": b.role, "members": b.members})).collect::<Vec<_>>(),
        "etag": policy.etag,
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

fn parse_labels(value: &Value) -> HashMap<String, String> {
    value
        .get("labels")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn parse_duration_seconds(value: Option<&Value>) -> Option<u32> {
    let raw = value?.as_str()?;
    raw.strip_suffix('s')?
        .parse::<f64>()
        .ok()
        .map(|v| v.max(0.0) as u32)
}

fn parse_push_config(value: &Value) -> (Option<String>, HashMap<String, String>) {
    let push = value.get("pushConfig");
    let endpoint = push
        .and_then(|v| v.get("pushEndpoint"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let attributes = push
        .and_then(|v| v.get("attributes"))
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    (endpoint, attributes)
}

fn parse_dead_letter(value: &Value) -> Option<DeadLetterSpec> {
    value.get("deadLetterPolicy").and_then(|v| {
        Some(DeadLetterSpec {
            topic: v.get("deadLetterTopic")?.as_str()?.to_string(),
            max_delivery_attempts: v
                .get("maxDeliveryAttempts")
                .and_then(Value::as_u64)
                .unwrap_or(5) as u32,
        })
    })
}

fn parse_retry(value: &Value) -> Option<RetrySpec> {
    value.get("retryPolicy").map(|v| RetrySpec {
        minimum_backoff_seconds: parse_duration_seconds(v.get("minimumBackoff")).unwrap_or(0),
        maximum_backoff_seconds: parse_duration_seconds(v.get("maximumBackoff")).unwrap_or(0),
    })
}

fn topic_spec_from_value(name: &str, value: &Value) -> TopicSpec {
    TopicSpec {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(name)
            .to_string(),
        labels: parse_labels(value),
        kms_key_name: value
            .get("kmsKeyName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        schema_name: value
            .get("schemaSettings")
            .and_then(|v| v.get("schema"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        schema_encoding: value
            .get("schemaSettings")
            .and_then(|v| v.get("encoding"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    }
}

fn subscription_spec_from_value(name: &str, value: &Value) -> Result<SubscriptionSpec, Response> {
    let topic = value
        .get("topic")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request("topic is required"))?;
    let (push_endpoint, push_attributes) = parse_push_config(value);
    Ok(SubscriptionSpec {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(name)
            .to_string(),
        topic: topic.to_string(),
        ack_deadline_seconds: value
            .get("ackDeadlineSeconds")
            .and_then(Value::as_u64)
            .unwrap_or(10) as u32,
        labels: parse_labels(value),
        enable_message_ordering: value
            .get("enableMessageOrdering")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        enable_exactly_once_delivery: value
            .get("enableExactlyOnceDelivery")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        dead_letter: parse_dead_letter(value),
        retry: parse_retry(value),
        push_endpoint,
        push_attributes,
        filter: value
            .get("filter")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

fn schema_spec_from_value(name: &str, value: &Value) -> Result<SchemaSpec, Response> {
    Ok(SchemaSpec {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(name)
            .to_string(),
        schema_type: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("UNSPECIFIED")
            .to_string(),
        definition: value
            .get("definition")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

fn policy_from_value(value: &Value) -> Result<IamPolicy, Response> {
    let bindings = value
        .get("bindings")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|b| {
                    Some(IamBinding {
                        role: b.get("role")?.as_str()?.to_string(),
                        members: b
                            .get("members")
                            .and_then(Value::as_array)?
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToString::to_string)
                            .collect(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(IamPolicy {
        version: value.get("version").and_then(Value::as_i64).unwrap_or(1) as i32,
        bindings,
        etag: value
            .get("etag")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

fn parse_update_mask(value: &Value, query: Option<&str>) -> Vec<String> {
    if let Some(mask) = query.filter(|s| !s.is_empty()) {
        return mask
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect();
    }
    if let Some(mask) = value.get("updateMask").and_then(Value::as_str) {
        return mask
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect();
    }
    value
        .get("updateMask")
        .and_then(|v| v.get("paths"))
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            Some(v)
        } else {
            None
        }
    })
}

fn page_size_from_query(query: Option<&str>) -> i32 {
    query
        .and_then(|q| query_param(q, "pageSize").or_else(|| query_param(q, "page_size")))
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
}

fn page_token_from_query(query: Option<&str>) -> String {
    query
        .and_then(|q| query_param(q, "pageToken").or_else(|| query_param(q, "page_token")))
        .unwrap_or("")
        .to_string()
}

fn auth_resource_for_path(path: &str, body: &Value) -> String {
    if let Some((resource, _)) = path.split_once(':') {
        return resource.to_string();
    }
    if path.ends_with("/topics") || path.ends_with("/subscriptions") || path.ends_with("/snapshots")
    {
        if let Some(project) = path
            .strip_prefix("projects/")
            .and_then(|p| p.split('/').next())
        {
            return format!("projects/{project}");
        }
    }
    if path.contains("/topics/") && path.ends_with("/subscriptions") {
        return path.trim_end_matches("/subscriptions").to_string();
    }
    if path.contains("/schemas") {
        if let Some(parent) = path.split(':').next() {
            if parent.ends_with("/schemas") {
                return parent.to_string();
            }
        }
    }
    if let Some(resource) = body.get("resource").and_then(Value::as_str) {
        return resource.to_string();
    }
    path.to_string()
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
                message_id: m
                    .get("messageId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

async fn google_dispatch(
    State(state): State<HttpState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let timer = state
        .metrics
        .latency
        .with_label_values(&["rest", "dispatch"])
        .start_timer();

    let path = uri.path().trim_start_matches("/v1/");
    let query = uri.query().unwrap_or("");
    let value: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return bad_request(format!("invalid json: {e}")),
        }
    };

    let resource = auth_resource_for_path(path, &value);
    if let Err(resp) = authorize(&state, &headers, &resource).await {
        timer.observe_duration();
        return resp;
    }

    let response = if method == Method::POST && path.ends_with(":publish") {
        handle_publish(&state, path, &value).await
    } else if method == Method::POST && path.ends_with(":pull") {
        handle_pull(&state, path, &value).await
    } else if method == Method::POST && path.ends_with(":acknowledge") {
        handle_acknowledge(&state, path, &value).await
    } else if method == Method::POST && path.ends_with(":modifyAckDeadline") {
        handle_modify_ack_deadline(&state, path, &value).await
    } else if method == Method::POST && path.ends_with(":seek") {
        handle_seek(&state, path, &value).await
    } else if method == Method::POST && path.ends_with(":modifyPushConfig") {
        handle_modify_push_config(&state, path, &value).await
    } else if method == Method::POST && path.ends_with(":getIamPolicy") {
        handle_get_iam_policy(&state, path).await
    } else if method == Method::POST && path.ends_with(":setIamPolicy") {
        handle_set_iam_policy(&state, path, &value).await
    } else if method == Method::POST && path.ends_with(":testIamPermissions") {
        handle_test_iam_permissions(&state, path, &value).await
    } else if method == Method::POST && path.ends_with(":validate") {
        handle_validate_schema(&state, path, &value).await
    } else if method == Method::POST && path.ends_with(":validateMessage") {
        handle_validate_message(&state, path, &value).await
    } else {
        handle_resource_request(&state, method, path, query, value).await
    };

    timer.observe_duration();
    response
}

async fn handle_publish(state: &HttpState, path: &str, value: &Value) -> Response {
    let topic = path.trim_end_matches(":publish");
    match parse_messages(value) {
        Ok(messages) => {
            let count = messages.len();
            match state.backend.publish(topic, messages).await {
                Ok(ids) => {
                    state
                        .metrics
                        .messages
                        .with_label_values(&["published"])
                        .inc_by(count as u64);
                    info!(topic = %topic, count, "published messages");
                    Json(json!({"messageIds": ids})).into_response()
                }
                Err(e) => backend_error(e),
            }
        }
        Err(e) => e,
    }
}

async fn handle_pull(state: &HttpState, path: &str, value: &Value) -> Response {
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
            info!(subscription = %subscription, count = deliveries.len(), "pulled messages");
            Json(json!({
                "receivedMessages": deliveries.into_iter().map(delivery_json).collect::<Vec<_>>()
            }))
            .into_response()
        }
        Err(e) => backend_error(e),
    }
}

async fn handle_acknowledge(state: &HttpState, path: &str, value: &Value) -> Response {
    let subscription = path.trim_end_matches(":acknowledge");
    let ack_ids: Vec<String> = value
        .get("ackIds")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    match state.backend.acknowledge(subscription, &ack_ids).await {
        Ok(()) => {
            info!(subscription = %subscription, count = ack_ids.len(), "acknowledged messages");
            Json(json!({})).into_response()
        }
        Err(e) => backend_error(e),
    }
}

async fn handle_modify_ack_deadline(state: &HttpState, path: &str, value: &Value) -> Response {
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
        return bad_request("ackDeadlineSeconds must be <= 600");
    }
    match state
        .backend
        .modify_ack_deadline(subscription, &ack_ids, seconds as u32)
        .await
    {
        Ok(()) => Json(json!({})).into_response(),
        Err(e) => backend_error(e),
    }
}

async fn handle_seek(state: &HttpState, path: &str, value: &Value) -> Response {
    let subscription = path.trim_end_matches(":seek");
    if let Some(snapshot) = value.get("snapshot").and_then(Value::as_str) {
        match state.backend.seek_to_snapshot(subscription, snapshot).await {
            Ok(()) => return Json(json!({})).into_response(),
            Err(e) => return backend_error(e),
        }
    }
    if let Some(time) = value
        .get("time")
        .and_then(Value::as_str)
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc))
    {
        match state.backend.seek_to_time(subscription, time).await {
            Ok(()) => Json(json!({})).into_response(),
            Err(e) => backend_error(e),
        }
    } else {
        bad_request("time or snapshot is required")
    }
}

async fn handle_modify_push_config(state: &HttpState, path: &str, value: &Value) -> Response {
    let subscription = path.trim_end_matches(":modifyPushConfig");
    let (push_endpoint, push_attributes) = parse_push_config(value);
    match state
        .backend
        .modify_push_config(subscription, push_endpoint, push_attributes)
        .await
    {
        Ok(()) => Json(json!({})).into_response(),
        Err(e) => backend_error(e),
    }
}

async fn handle_get_iam_policy(state: &HttpState, path: &str) -> Response {
    let resource = path.trim_end_matches(":getIamPolicy");
    match state.backend.get_iam_policy(resource).await {
        Ok(policy) => Json(policy_json(policy)).into_response(),
        Err(e) => backend_error(e),
    }
}

async fn handle_set_iam_policy(state: &HttpState, path: &str, value: &Value) -> Response {
    let resource = path.trim_end_matches(":setIamPolicy");
    let policy = match policy_from_value(value.get("policy").unwrap_or(value)) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match state.backend.set_iam_policy(resource, policy).await {
        Ok(updated) => Json(policy_json(updated)).into_response(),
        Err(e) => backend_error(e),
    }
}

async fn handle_test_iam_permissions(state: &HttpState, path: &str, value: &Value) -> Response {
    let resource = path.trim_end_matches(":testIamPermissions");
    let permissions: Vec<String> = value
        .get("permissions")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    match state
        .backend
        .test_iam_permissions(resource, &permissions)
        .await
    {
        Ok(granted) => Json(json!({"permissions": granted})).into_response(),
        Err(e) => backend_error(e),
    }
}

async fn handle_validate_schema(state: &HttpState, path: &str, value: &Value) -> Response {
    let parent = path.trim_end_matches(":validate");
    let schema_body = value.get("schema").unwrap_or(value);
    let name = schema_body
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(parent);
    let spec = match schema_spec_from_value(name, schema_body) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match state.backend.validate_schema(&spec).await {
        Ok(()) => Json(json!({})).into_response(),
        Err(e) => backend_error(e),
    }
}

async fn handle_validate_message(state: &HttpState, path: &str, value: &Value) -> Response {
    let parent = path.trim_end_matches(":validateMessage");
    let raw = value.get("message").and_then(Value::as_str).unwrap_or("");
    let message = match BASE64.decode(raw) {
        Ok(b) => b,
        Err(_) => return bad_request("message must be base64"),
    };

    if let Some(name) = value
        .get("schema")
        .and_then(|v| v.get("name"))
        .and_then(Value::as_str)
    {
        match state
            .backend
            .validate_message(Some(name), None, &message)
            .await
        {
            Ok(()) => return Json(json!({})).into_response(),
            Err(e) => return backend_error(e),
        }
    }

    if let Some(schema_body) = value.get("schema") {
        let spec = match schema_spec_from_value(parent, schema_body) {
            Ok(s) => s,
            Err(e) => return e,
        };
        match state
            .backend
            .validate_message(None, Some(&spec), &message)
            .await
        {
            Ok(()) => Json(json!({})).into_response(),
            Err(e) => backend_error(e),
        }
    } else {
        bad_request("schema or schema.name is required")
    }
}

async fn handle_resource_request(
    state: &HttpState,
    method: Method,
    path: &str,
    query: &str,
    value: Value,
) -> Response {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 3 || parts[0] != "projects" {
        return not_found("unknown Pub/Sub REST path");
    }
    let project = format!("projects/{}", parts[1]);
    let page_size = page_size_from_query(Some(query));
    let page_token = page_token_from_query(Some(query));
    let update_mask = parse_update_mask(&value, query_param(query, "updateMask"));

    if parts.len() == 3 && parts[2] == "topics" && method == Method::GET {
        return match state
            .backend
            .list_topics(&project, page_size, &page_token)
            .await
        {
            Ok(page) => Json(json!({
                "topics": page.items.into_iter().map(topic_json).collect::<Vec<_>>(),
                "nextPageToken": page.next_page_token,
            }))
            .into_response(),
            Err(e) => backend_error(e),
        };
    }

    if parts.len() == 3 && parts[2] == "subscriptions" && method == Method::GET {
        return match state
            .backend
            .list_subscriptions(&project, page_size, &page_token)
            .await
        {
            Ok(page) => Json(json!({
                "subscriptions": page.items.into_iter().map(subscription_json).collect::<Vec<_>>(),
                "nextPageToken": page.next_page_token,
            }))
            .into_response(),
            Err(e) => backend_error(e),
        };
    }

    if parts.len() == 3 && parts[2] == "snapshots" && method == Method::GET {
        return match state
            .backend
            .list_snapshots(&project, page_size, &page_token)
            .await
        {
            Ok(page) => Json(json!({
                "snapshots": page.items.into_iter().map(snapshot_json).collect::<Vec<_>>(),
                "nextPageToken": page.next_page_token,
            }))
            .into_response(),
            Err(e) => backend_error(e),
        };
    }

    if parts.len() == 3 && parts[2] == "schemas" && method == Method::GET {
        return match state
            .backend
            .list_schemas(&project, page_size, &page_token)
            .await
        {
            Ok(page) => Json(json!({
                "schemas": page.items.into_iter().map(schema_json).collect::<Vec<_>>(),
                "nextPageToken": page.next_page_token,
            }))
            .into_response(),
            Err(e) => backend_error(e),
        };
    }

    if parts.len() == 5
        && parts[2] == "topics"
        && parts[4] == "subscriptions"
        && method == Method::GET
    {
        let topic = path.trim_end_matches("/subscriptions");
        return match state
            .backend
            .list_topic_subscriptions(topic, page_size, &page_token)
            .await
        {
            Ok(page) => Json(json!({
                "subscriptions": page.items,
                "nextPageToken": page.next_page_token,
            }))
            .into_response(),
            Err(e) => backend_error(e),
        };
    }

    if parts.len() == 4 && parts[2] == "topics" {
        let name = path.to_string();
        return match method {
            Method::PUT => {
                let spec = topic_spec_from_value(&name, &value);
                match state.backend.create_topic(spec).await {
                    Ok(topic) => (StatusCode::OK, Json(topic_json(topic))).into_response(),
                    Err(e) => backend_error(e),
                }
            }
            Method::PATCH => {
                let spec = topic_spec_from_value(&name, &value);
                match state.backend.update_topic(spec, &update_mask).await {
                    Ok(topic) => Json(topic_json(topic)).into_response(),
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
            Method::PUT => match subscription_spec_from_value(&name, &value) {
                Ok(spec) => match state.backend.create_subscription(spec).await {
                    Ok(sub) => Json(subscription_json(sub)).into_response(),
                    Err(e) => backend_error(e),
                },
                Err(e) => e,
            },
            Method::PATCH => match subscription_spec_from_value(&name, &value) {
                Ok(spec) => match state.backend.update_subscription(spec, &update_mask).await {
                    Ok(sub) => Json(subscription_json(sub)).into_response(),
                    Err(e) => backend_error(e),
                },
                Err(e) => e,
            },
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

    if parts.len() == 4 && parts[2] == "snapshots" {
        let name = path.to_string();
        return match method {
            Method::PUT => {
                let Some(subscription) = value.get("subscription").and_then(Value::as_str) else {
                    return bad_request("subscription is required");
                };
                let labels = parse_labels(&value);
                match state
                    .backend
                    .create_snapshot(&name, subscription, labels)
                    .await
                {
                    Ok(snapshot) => Json(snapshot_json(snapshot)).into_response(),
                    Err(e) => backend_error(e),
                }
            }
            Method::PATCH => {
                let expire_time = value
                    .get("expireTime")
                    .and_then(Value::as_str)
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now);
                let spec = SnapshotSpec {
                    name: value
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&name)
                        .to_string(),
                    topic: value
                        .get("topic")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    expire_time,
                    labels: parse_labels(&value),
                    topic_index: 0,
                };
                match state.backend.update_snapshot(spec, &update_mask).await {
                    Ok(snapshot) => Json(snapshot_json(snapshot)).into_response(),
                    Err(e) => backend_error(e),
                }
            }
            Method::GET => match state.backend.get_snapshot(&name).await {
                Ok(snapshot) => Json(snapshot_json(snapshot)).into_response(),
                Err(e) => backend_error(e),
            },
            Method::DELETE => match state.backend.delete_snapshot(&name).await {
                Ok(()) => Json(json!({})).into_response(),
                Err(e) => backend_error(e),
            },
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        };
    }

    if parts.len() == 4 && parts[2] == "schemas" {
        let name = path.to_string();
        return match method {
            Method::PUT => match schema_spec_from_value(&name, &value) {
                Ok(spec) => match state.backend.create_schema(spec).await {
                    Ok(schema) => Json(schema_json(schema)).into_response(),
                    Err(e) => backend_error(e),
                },
                Err(e) => e,
            },
            Method::GET => match state.backend.get_schema(&name).await {
                Ok(schema) => Json(schema_json(schema)).into_response(),
                Err(e) => backend_error(e),
            },
            Method::DELETE => match state.backend.delete_schema(&name).await {
                Ok(()) => Json(json!({})).into_response(),
                Err(e) => backend_error(e),
            },
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        };
    }

    not_found("unknown Pub/Sub REST path")
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
    let project = query.project.as_deref().unwrap_or("projects/demo");
    if let Err(resp) = authorize(&state, &headers, project).await {
        return resp;
    }
    match state.backend.list_topics(project, 1000, "").await {
        Ok(page) => Json(page.items).into_response(),
        Err(e) => backend_error(e),
    }
}

async fn admin_subscriptions(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(query): Query<ProjectQuery>,
) -> Response {
    let project = query.project.as_deref().unwrap_or("projects/demo");
    if let Err(resp) = authorize(&state, &headers, project).await {
        return resp;
    }
    match state.backend.list_subscriptions(project, 1000, "").await {
        Ok(page) => Json(page.items).into_response(),
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
    #[serde(default)]
    message_id: String,
}

async fn admin_publish(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(req): Json<AdminPublish>,
) -> Response {
    if let Err(resp) = authorize(&state, &headers, &req.topic).await {
        return resp;
    }
    match state
        .backend
        .publish(
            &req.topic,
            vec![NewMessage {
                data: req.data.into_bytes(),
                attributes: req.attributes,
                ordering_key: req.ordering_key,
                message_id: req.message_id,
            }],
        )
        .await
    {
        Ok(ids) => {
            info!(topic = %req.topic, count = 1, id = %ids.first().unwrap_or(&String::new()), "admin publish");
            Json(json!({"message_ids": ids})).into_response()
        }
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
    if let Err(resp) = authorize(&state, &headers, &req.subscription).await {
        return resp;
    }
    match state
        .backend
        .pull(&req.subscription, req.max_messages)
        .await
    {
        Ok(v) => {
            info!(subscription = %req.subscription, count = v.len(), "admin pull");
            Json(v).into_response()
        }
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
    if let Err(resp) = authorize(&state, &headers, &req.subscription).await {
        return resp;
    }
    match state
        .backend
        .acknowledge(&req.subscription, &req.ack_ids)
        .await
    {
        Ok(()) => {
            info!(
                subscription = %req.subscription,
                count = req.ack_ids.len(),
                "admin ack"
            );
            Json(json!({"ok":true})).into_response()
        }
        Err(e) => backend_error(e),
    }
}

async fn admin_inventory(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(query): Query<ProjectQuery>,
) -> Response {
    let project = query.project.as_deref().unwrap_or("projects/demo");
    if let Err(resp) = authorize(&state, &headers, project).await {
        return resp;
    }
    match state.backend.inventory(project).await {
        Ok(report) => Json(report).into_response(),
        Err(e) => backend_error(e),
    }
}

#[derive(Deserialize)]
struct AdminPushConfig {
    subscription: String,
    #[serde(default)]
    push_endpoint: Option<String>,
    #[serde(default)]
    push_attributes: HashMap<String, String>,
}

async fn admin_push_config(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(req): Json<AdminPushConfig>,
) -> Response {
    if let Err(resp) = authorize(&state, &headers, &req.subscription).await {
        return resp;
    }
    let endpoint = req
        .push_endpoint
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match state
        .backend
        .modify_push_config(&req.subscription, endpoint.clone(), req.push_attributes)
        .await
    {
        Ok(()) => {
            info!(
                subscription = %req.subscription,
                push_endpoint = endpoint.as_deref().unwrap_or("(cleared)"),
                "admin push-config updated"
            );
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => backend_error(e),
    }
}

#[derive(Deserialize)]
struct LogsQuery {
    #[serde(default)]
    after: Option<u64>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn admin_logs(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(query): Query<LogsQuery>,
) -> Response {
    if let Err(resp) = authorize(&state, &headers, "projects/demo").await {
        return resp;
    }
    let limit = query.limit.unwrap_or(200);
    let entries = match query.after {
        Some(after) => state.logs.since(after, limit),
        None => state.logs.recent(limit),
    };
    Json(json!({
        "entries": entries,
        "next_after": entries.last().map(|e| e.id).unwrap_or(query.after.unwrap_or(0)),
    }))
    .into_response()
}
