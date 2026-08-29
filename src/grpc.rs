// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

use crate::auth::Authenticator;
use crate::backend::{BackendError, RelayBackend};
use crate::google::pubsub::v1::schema_service_server::SchemaService;
use crate::google::pubsub::v1::validate_message_request::SchemaSpec as ValidateMessageSchemaSpec;
use crate::google::pubsub::v1::{
    publisher_server::Publisher, seek_request, subscriber_server::Subscriber, AcknowledgeRequest,
    Binding, CreateSchemaRequest, CreateSnapshotRequest, DeleteSchemaRequest,
    DeleteSnapshotRequest, DeleteSubscriptionRequest, DeleteTopicRequest, Empty,
    GetIamPolicyRequest, GetSchemaRequest, GetSnapshotRequest, GetSubscriptionRequest,
    GetTopicRequest, ListSchemasRequest, ListSchemasResponse, ListSnapshotsRequest,
    ListSnapshotsResponse, ListSubscriptionsRequest, ListSubscriptionsResponse,
    ListTopicSubscriptionsRequest, ListTopicSubscriptionsResponse, ListTopicsRequest,
    ListTopicsResponse, ModifyAckDeadlineRequest, ModifyPushConfigRequest, Policy, PublishRequest,
    PublishResponse, PubsubMessage, PullRequest, PullResponse, PushConfig, ReceivedMessage, Schema,
    SchemaType, SeekRequest, SeekResponse, SetIamPolicyRequest, Snapshot, StreamingPullRequest,
    StreamingPullResponse, Subscription, TestIamPermissionsRequest, TestIamPermissionsResponse,
    Topic, UpdateSnapshotRequest, UpdateSubscriptionRequest, UpdateTopicRequest,
    ValidateMessageRequest, ValidateMessageResponse, ValidateSchemaRequest, ValidateSchemaResponse,
};
use crate::metrics::Metrics;
use crate::model::{
    DeadLetterSpec, Delivery, IamBinding, IamPolicy, NewMessage, RetrySpec, SchemaSpec,
    SnapshotSpec, SubscriptionSpec, TopicSpec,
};
use chrono::{DateTime, Utc};
use futures::Stream;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct GatewayService {
    backend: Arc<dyn RelayBackend>,
    authenticator: Option<Authenticator>,
    metrics: Metrics,
}

impl GatewayService {
    pub fn new(
        backend: Arc<dyn RelayBackend>,
        authenticator: Option<Authenticator>,
        metrics: Metrics,
    ) -> Self {
        Self {
            backend,
            authenticator,
            metrics,
        }
    }

    async fn authorize<T>(&self, request: &Request<T>, resource: &str) -> Result<(), Status> {
        let Some(auth) = &self.authenticator else {
            return Ok(());
        };
        let header = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        let ctx = auth
            .authenticate_bearer(header)
            .await
            .map_err(Status::unauthenticated)?;
        auth.authorize_resource(&ctx, resource)
            .map_err(Status::permission_denied)?;
        Ok(())
    }

    fn record(&self, transport: &str, operation: &str, result: &str) {
        self.metrics
            .requests
            .with_label_values(&[transport, operation, result])
            .inc();
    }
}

fn backend_status(error: BackendError) -> Status {
    match error {
        BackendError::NotFound(v) => Status::not_found(v),
        BackendError::AlreadyExists(v) => Status::already_exists(v),
        BackendError::InvalidArgument(v) => Status::invalid_argument(v),
        BackendError::FailedPrecondition(v) => Status::failed_precondition(v),
        BackendError::PermissionDenied(v) => Status::permission_denied(v),
        BackendError::Unavailable(v) => Status::unavailable(v),
        BackendError::Internal(v) => Status::internal(v),
    }
}

fn field_mask_paths(mask: Option<prost_types::FieldMask>) -> Vec<String> {
    mask.map(|m| m.paths).unwrap_or_default()
}

fn topic_to_proto(value: TopicSpec) -> Topic {
    Topic {
        name: value.name,
        labels: value.labels,
        kms_key_name: value.kms_key_name,
    }
}

fn topic_from_proto(value: Topic) -> Result<TopicSpec, Status> {
    if value.name.is_empty() {
        return Err(Status::invalid_argument("topic name is required"));
    }
    Ok(TopicSpec {
        name: value.name,
        labels: value.labels,
        kms_key_name: value.kms_key_name,
    })
}

fn subscription_to_proto(value: SubscriptionSpec) -> Subscription {
    let push_config = value.push_endpoint.as_ref().map(|endpoint| PushConfig {
        push_endpoint: endpoint.clone(),
        attributes: value.push_attributes.clone(),
    });
    Subscription {
        name: value.name,
        topic: value.topic,
        push_config,
        ack_deadline_seconds: value.ack_deadline_seconds as i32,
        retain_acked_messages: false,
        labels: value.labels,
        enable_message_ordering: value.enable_message_ordering,
        dead_letter_policy: value.dead_letter.map(|v| {
            crate::google::pubsub::v1::DeadLetterPolicy {
                dead_letter_topic: v.topic,
                max_delivery_attempts: v.max_delivery_attempts as i32,
            }
        }),
        retry_policy: value.retry.map(|v| crate::google::pubsub::v1::RetryPolicy {
            minimum_backoff: Some(prost_types::Duration {
                seconds: v.minimum_backoff_seconds as i64,
                nanos: 0,
            }),
            maximum_backoff: Some(prost_types::Duration {
                seconds: v.maximum_backoff_seconds as i64,
                nanos: 0,
            }),
        }),
        detached: false,
        enable_exactly_once_delivery: value.enable_exactly_once_delivery,
    }
}

fn subscription_from_proto(value: Subscription) -> Result<SubscriptionSpec, Status> {
    if value.name.is_empty() {
        return Err(Status::invalid_argument("subscription name is required"));
    }
    if value.topic.is_empty() {
        return Err(Status::invalid_argument("subscription topic is required"));
    }
    let (push_endpoint, push_attributes) = value
        .push_config
        .map(|v| {
            let endpoint = if v.push_endpoint.is_empty() {
                None
            } else {
                Some(v.push_endpoint)
            };
            (endpoint, v.attributes)
        })
        .unwrap_or((None, HashMap::new()));
    Ok(SubscriptionSpec {
        name: value.name,
        topic: value.topic,
        ack_deadline_seconds: if value.ack_deadline_seconds <= 0 {
            10
        } else {
            value.ack_deadline_seconds as u32
        },
        labels: value.labels,
        enable_message_ordering: value.enable_message_ordering,
        enable_exactly_once_delivery: value.enable_exactly_once_delivery,
        dead_letter: value.dead_letter_policy.map(|v| DeadLetterSpec {
            topic: v.dead_letter_topic,
            max_delivery_attempts: v.max_delivery_attempts.max(5) as u32,
        }),
        retry: value.retry_policy.map(|v| RetrySpec {
            minimum_backoff_seconds: v
                .minimum_backoff
                .map(|d| d.seconds.max(0) as u32)
                .unwrap_or(0),
            maximum_backoff_seconds: v
                .maximum_backoff
                .map(|d| d.seconds.max(0) as u32)
                .unwrap_or(0),
        }),
        push_endpoint,
        push_attributes,
    })
}

fn snapshot_to_proto(value: SnapshotSpec) -> Snapshot {
    Snapshot {
        name: value.name,
        topic: value.topic,
        expire_time: Some(prost_types::Timestamp {
            seconds: value.expire_time.timestamp(),
            nanos: value.expire_time.timestamp_subsec_nanos() as i32,
        }),
        labels: value.labels,
    }
}

fn snapshot_from_proto(value: Snapshot) -> Result<SnapshotSpec, Status> {
    if value.name.is_empty() {
        return Err(Status::invalid_argument("snapshot name is required"));
    }
    let expire_time = match value.expire_time {
        Some(ts) => timestamp_to_datetime(ts)?,
        None => Utc::now(),
    };
    Ok(SnapshotSpec {
        name: value.name,
        topic: value.topic,
        expire_time,
        labels: value.labels,
        topic_index: 0,
    })
}

fn schema_type_to_proto(value: &str) -> i32 {
    match value {
        "PROTOCOL_BUFFER" => SchemaType::ProtocolBuffer as i32,
        "AVRO" => SchemaType::Avro as i32,
        _ => SchemaType::Unspecified as i32,
    }
}

fn schema_type_from_proto(value: i32) -> String {
    match SchemaType::try_from(value).unwrap_or(SchemaType::Unspecified) {
        SchemaType::ProtocolBuffer => "PROTOCOL_BUFFER".into(),
        SchemaType::Avro => "AVRO".into(),
        SchemaType::Unspecified => "UNSPECIFIED".into(),
    }
}

fn schema_to_proto(value: SchemaSpec) -> Schema {
    Schema {
        name: value.name,
        r#type: schema_type_to_proto(&value.schema_type),
        definition: value.definition,
    }
}

fn schema_from_proto(value: Schema) -> Result<SchemaSpec, Status> {
    if value.name.is_empty() {
        return Err(Status::invalid_argument("schema name is required"));
    }
    Ok(SchemaSpec {
        name: value.name,
        schema_type: schema_type_from_proto(value.r#type),
        definition: value.definition,
    })
}

fn policy_to_proto(value: IamPolicy) -> Policy {
    Policy {
        version: value.version,
        bindings: value
            .bindings
            .into_iter()
            .map(|b| Binding {
                role: b.role,
                members: b.members,
            })
            .collect(),
        etag: value.etag,
    }
}

fn policy_from_proto(value: Policy) -> IamPolicy {
    IamPolicy {
        version: value.version,
        bindings: value
            .bindings
            .into_iter()
            .map(|b| IamBinding {
                role: b.role,
                members: b.members,
            })
            .collect(),
        etag: value.etag,
    }
}

fn delivery_to_proto(value: Delivery) -> ReceivedMessage {
    let published = value.message.published_at;
    ReceivedMessage {
        ack_id: value.ack_id,
        message: Some(PubsubMessage {
            data: value.message.data,
            attributes: value.message.attributes,
            message_id: value.message.id,
            publish_time: Some(prost_types::Timestamp {
                seconds: published.timestamp(),
                nanos: published.timestamp_subsec_nanos() as i32,
            }),
            ordering_key: value.message.ordering_key,
        }),
        delivery_attempt: value.delivery_attempt as i32,
    }
}

fn timestamp_to_datetime(ts: prost_types::Timestamp) -> Result<DateTime<Utc>, Status> {
    if ts.nanos < 0 || ts.nanos >= 1_000_000_000 {
        return Err(Status::invalid_argument("invalid timestamp nanos"));
    }
    DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
        .ok_or_else(|| Status::invalid_argument("invalid timestamp"))
}

#[tonic::async_trait]
impl Publisher for GatewayService {
    async fn create_topic(&self, request: Request<Topic>) -> Result<Response<Topic>, Status> {
        self.authorize(&request, &request.get_ref().name).await?;
        let timer = self
            .metrics
            .latency
            .with_label_values(&["grpc", "CreateTopic"])
            .start_timer();
        let result = self
            .backend
            .create_topic(topic_from_proto(request.into_inner())?)
            .await;
        timer.observe_duration();
        match result {
            Ok(topic) => {
                self.record("grpc", "CreateTopic", "ok");
                Ok(Response::new(topic_to_proto(topic)))
            }
            Err(e) => {
                self.record("grpc", "CreateTopic", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn update_topic(
        &self,
        request: Request<UpdateTopicRequest>,
    ) -> Result<Response<Topic>, Status> {
        let req = request.get_ref();
        let topic = req
            .topic
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("topic is required"))?;
        self.authorize(&request, &topic.name).await?;
        let spec = topic_from_proto(topic.clone())?;
        let update_mask = field_mask_paths(req.update_mask.clone());
        match self.backend.update_topic(spec, &update_mask).await {
            Ok(topic) => {
                self.record("grpc", "UpdateTopic", "ok");
                Ok(Response::new(topic_to_proto(topic)))
            }
            Err(e) => {
                self.record("grpc", "UpdateTopic", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn get_topic(
        &self,
        request: Request<GetTopicRequest>,
    ) -> Result<Response<Topic>, Status> {
        self.authorize(&request, &request.get_ref().topic).await?;
        let result = self.backend.get_topic(&request.into_inner().topic).await;
        match result {
            Ok(topic) => {
                self.record("grpc", "GetTopic", "ok");
                Ok(Response::new(topic_to_proto(topic)))
            }
            Err(e) => {
                self.record("grpc", "GetTopic", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn list_topics(
        &self,
        request: Request<ListTopicsRequest>,
    ) -> Result<Response<ListTopicsResponse>, Status> {
        self.authorize(&request, &request.get_ref().project).await?;
        let req = request.into_inner();
        match self
            .backend
            .list_topics(&req.project, req.page_size, &req.page_token)
            .await
        {
            Ok(page) => {
                self.record("grpc", "ListTopics", "ok");
                Ok(Response::new(ListTopicsResponse {
                    topics: page.items.into_iter().map(topic_to_proto).collect(),
                    next_page_token: page.next_page_token,
                }))
            }
            Err(e) => {
                self.record("grpc", "ListTopics", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn list_topic_subscriptions(
        &self,
        request: Request<ListTopicSubscriptionsRequest>,
    ) -> Result<Response<ListTopicSubscriptionsResponse>, Status> {
        self.authorize(&request, &request.get_ref().topic).await?;
        let req = request.into_inner();
        match self
            .backend
            .list_topic_subscriptions(&req.topic, req.page_size, &req.page_token)
            .await
        {
            Ok(page) => {
                self.record("grpc", "ListTopicSubscriptions", "ok");
                Ok(Response::new(ListTopicSubscriptionsResponse {
                    subscriptions: page.items,
                    next_page_token: page.next_page_token,
                }))
            }
            Err(e) => {
                self.record("grpc", "ListTopicSubscriptions", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn delete_topic(
        &self,
        request: Request<DeleteTopicRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.authorize(&request, &request.get_ref().topic).await?;
        match self.backend.delete_topic(&request.into_inner().topic).await {
            Ok(()) => {
                self.record("grpc", "DeleteTopic", "ok");
                Ok(Response::new(Empty {}))
            }
            Err(e) => {
                self.record("grpc", "DeleteTopic", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn publish(
        &self,
        request: Request<PublishRequest>,
    ) -> Result<Response<PublishResponse>, Status> {
        self.authorize(&request, &request.get_ref().topic).await?;
        let req = request.into_inner();
        if req.messages.is_empty() {
            return Err(Status::invalid_argument("at least one message is required"));
        }
        let count = req.messages.len();
        let messages = req
            .messages
            .into_iter()
            .map(|m| NewMessage {
                data: m.data,
                attributes: m.attributes,
                ordering_key: m.ordering_key,
            })
            .collect();
        match self.backend.publish(&req.topic, messages).await {
            Ok(message_ids) => {
                self.metrics
                    .messages
                    .with_label_values(&["published"])
                    .inc_by(count as u64);
                self.record("grpc", "Publish", "ok");
                Ok(Response::new(PublishResponse { message_ids }))
            }
            Err(e) => {
                self.record("grpc", "Publish", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn get_iam_policy(
        &self,
        request: Request<GetIamPolicyRequest>,
    ) -> Result<Response<Policy>, Status> {
        self.authorize(&request, &request.get_ref().resource)
            .await?;
        match self
            .backend
            .get_iam_policy(&request.into_inner().resource)
            .await
        {
            Ok(policy) => {
                self.record("grpc", "GetIamPolicy", "ok");
                Ok(Response::new(policy_to_proto(policy)))
            }
            Err(e) => {
                self.record("grpc", "GetIamPolicy", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn set_iam_policy(
        &self,
        request: Request<SetIamPolicyRequest>,
    ) -> Result<Response<Policy>, Status> {
        self.authorize(&request, &request.get_ref().resource)
            .await?;
        let req = request.into_inner();
        let policy = req
            .policy
            .ok_or_else(|| Status::invalid_argument("policy is required"))?;
        match self
            .backend
            .set_iam_policy(&req.resource, policy_from_proto(policy))
            .await
        {
            Ok(updated) => {
                self.record("grpc", "SetIamPolicy", "ok");
                Ok(Response::new(policy_to_proto(updated)))
            }
            Err(e) => {
                self.record("grpc", "SetIamPolicy", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn test_iam_permissions(
        &self,
        request: Request<TestIamPermissionsRequest>,
    ) -> Result<Response<TestIamPermissionsResponse>, Status> {
        self.authorize(&request, &request.get_ref().resource)
            .await?;
        let req = request.into_inner();
        match self
            .backend
            .test_iam_permissions(&req.resource, &req.permissions)
            .await
        {
            Ok(granted) => {
                self.record("grpc", "TestIamPermissions", "ok");
                Ok(Response::new(TestIamPermissionsResponse {
                    permissions: granted,
                }))
            }
            Err(e) => {
                self.record("grpc", "TestIamPermissions", "error");
                Err(backend_status(e))
            }
        }
    }
}

#[tonic::async_trait]
impl Subscriber for GatewayService {
    async fn create_subscription(
        &self,
        request: Request<Subscription>,
    ) -> Result<Response<Subscription>, Status> {
        self.authorize(&request, &request.get_ref().name).await?;
        let spec = subscription_from_proto(request.into_inner())?;
        match self.backend.create_subscription(spec).await {
            Ok(sub) => {
                self.record("grpc", "CreateSubscription", "ok");
                Ok(Response::new(subscription_to_proto(sub)))
            }
            Err(e) => {
                self.record("grpc", "CreateSubscription", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn update_subscription(
        &self,
        request: Request<UpdateSubscriptionRequest>,
    ) -> Result<Response<Subscription>, Status> {
        let req = request.get_ref();
        let subscription = req
            .subscription
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("subscription is required"))?;
        self.authorize(&request, &subscription.name).await?;
        let spec = subscription_from_proto(subscription.clone())?;
        let update_mask = field_mask_paths(req.update_mask.clone());
        match self.backend.update_subscription(spec, &update_mask).await {
            Ok(sub) => {
                self.record("grpc", "UpdateSubscription", "ok");
                Ok(Response::new(subscription_to_proto(sub)))
            }
            Err(e) => {
                self.record("grpc", "UpdateSubscription", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn get_subscription(
        &self,
        request: Request<GetSubscriptionRequest>,
    ) -> Result<Response<Subscription>, Status> {
        self.authorize(&request, &request.get_ref().subscription)
            .await?;
        match self
            .backend
            .get_subscription(&request.into_inner().subscription)
            .await
        {
            Ok(sub) => {
                self.record("grpc", "GetSubscription", "ok");
                Ok(Response::new(subscription_to_proto(sub)))
            }
            Err(e) => {
                self.record("grpc", "GetSubscription", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn list_subscriptions(
        &self,
        request: Request<ListSubscriptionsRequest>,
    ) -> Result<Response<ListSubscriptionsResponse>, Status> {
        self.authorize(&request, &request.get_ref().project).await?;
        let req = request.into_inner();
        match self
            .backend
            .list_subscriptions(&req.project, req.page_size, &req.page_token)
            .await
        {
            Ok(page) => {
                self.record("grpc", "ListSubscriptions", "ok");
                Ok(Response::new(ListSubscriptionsResponse {
                    subscriptions: page.items.into_iter().map(subscription_to_proto).collect(),
                    next_page_token: page.next_page_token,
                }))
            }
            Err(e) => {
                self.record("grpc", "ListSubscriptions", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn delete_subscription(
        &self,
        request: Request<DeleteSubscriptionRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.authorize(&request, &request.get_ref().subscription)
            .await?;
        match self
            .backend
            .delete_subscription(&request.into_inner().subscription)
            .await
        {
            Ok(()) => {
                self.record("grpc", "DeleteSubscription", "ok");
                Ok(Response::new(Empty {}))
            }
            Err(e) => {
                self.record("grpc", "DeleteSubscription", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn modify_push_config(
        &self,
        request: Request<ModifyPushConfigRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.authorize(&request, &request.get_ref().subscription)
            .await?;
        let req = request.into_inner();
        let (push_endpoint, push_attributes) = req
            .push_config
            .map(|cfg| {
                let endpoint = if cfg.push_endpoint.is_empty() {
                    None
                } else {
                    Some(cfg.push_endpoint)
                };
                (endpoint, cfg.attributes)
            })
            .unwrap_or((None, HashMap::new()));
        match self
            .backend
            .modify_push_config(&req.subscription, push_endpoint, push_attributes)
            .await
        {
            Ok(()) => {
                self.record("grpc", "ModifyPushConfig", "ok");
                Ok(Response::new(Empty {}))
            }
            Err(e) => {
                self.record("grpc", "ModifyPushConfig", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn pull(&self, request: Request<PullRequest>) -> Result<Response<PullResponse>, Status> {
        self.authorize(&request, &request.get_ref().subscription)
            .await?;
        let req = request.into_inner();
        match self
            .backend
            .pull(&req.subscription, req.max_messages.max(1) as u32)
            .await
        {
            Ok(deliveries) => {
                self.metrics
                    .messages
                    .with_label_values(&["delivered"])
                    .inc_by(deliveries.len() as u64);
                self.record("grpc", "Pull", "ok");
                Ok(Response::new(PullResponse {
                    received_messages: deliveries.into_iter().map(delivery_to_proto).collect(),
                }))
            }
            Err(e) => {
                self.record("grpc", "Pull", "error");
                Err(backend_status(e))
            }
        }
    }

    type StreamingPullStream =
        Pin<Box<dyn Stream<Item = Result<StreamingPullResponse, Status>> + Send + 'static>>;

    async fn streaming_pull(
        &self,
        request: Request<tonic::Streaming<StreamingPullRequest>>,
    ) -> Result<Response<Self::StreamingPullStream>, Status> {
        let auth_ctx = if let Some(auth) = &self.authenticator {
            let header = request
                .metadata()
                .get("authorization")
                .and_then(|v| v.to_str().ok());
            Some(
                auth.authenticate_bearer(header)
                    .await
                    .map_err(Status::unauthenticated)?,
            )
        } else {
            None
        };

        let mut input = request.into_inner();
        let backend = self.backend.clone();
        let metrics = self.metrics.clone();
        let authenticator = self.authenticator.clone();
        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            let mut subscription = String::new();
            let mut batch_size = 100u32;
            let mut authorized = false;
            let mut interval = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    incoming = input.message() => {
                        match incoming {
                            Ok(Some(frame)) => {
                                if !frame.subscription.is_empty() {
                                    subscription = frame.subscription.clone();
                                }
                                if frame.max_outstanding_messages > 0 {
                                    batch_size = frame.max_outstanding_messages.min(1000) as u32;
                                }
                                if subscription.is_empty() { continue; }
                                if !authorized {
                                    if let (Some(auth), Some(ctx)) = (&authenticator, &auth_ctx) {
                                        if let Err(status) = auth
                                            .authorize_resource(ctx, &subscription)
                                            .map_err(Status::permission_denied)
                                        {
                                            let _ = tx.send(Err(status)).await;
                                            break;
                                        }
                                    }
                                    authorized = true;
                                }
                                if !frame.ack_ids.is_empty() {
                                    if let Err(e) = backend.acknowledge(&subscription, &frame.ack_ids).await {
                                        let _ = tx.send(Err(backend_status(e))).await;
                                        break;
                                    }
                                }
                                for (ack_id, deadline) in frame.modify_deadline_ack_ids.iter().zip(frame.modify_deadline_seconds.iter()) {
                                    if let Err(e) = backend.modify_ack_deadline(
                                        &subscription,
                                        std::slice::from_ref(ack_id),
                                        (*deadline).max(0) as u32,
                                    ).await {
                                        let _ = tx.send(Err(backend_status(e))).await;
                                        return;
                                    }
                                }
                            }
                            Ok(None) => break,
                            Err(status) => {
                                let _ = tx.send(Err(status)).await;
                                break;
                            }
                        }
                    }
                    _ = interval.tick(), if !subscription.is_empty() && authorized => {
                        match backend.pull(&subscription, batch_size).await {
                            Ok(deliveries) if !deliveries.is_empty() => {
                                metrics.messages.with_label_values(&["delivered"]).inc_by(deliveries.len() as u64);
                                let response = StreamingPullResponse {
                                    received_messages: deliveries.into_iter().map(delivery_to_proto).collect(),
                                };
                                if tx.send(Ok(response)).await.is_err() { break; }
                            }
                            Ok(_) => {}
                            Err(e) => {
                                let _ = tx.send(Err(backend_status(e))).await;
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.record("grpc", "StreamingPull", "ok");
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn acknowledge(
        &self,
        request: Request<AcknowledgeRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.authorize(&request, &request.get_ref().subscription)
            .await?;
        let req = request.into_inner();
        match self
            .backend
            .acknowledge(&req.subscription, &req.ack_ids)
            .await
        {
            Ok(()) => {
                self.record("grpc", "Acknowledge", "ok");
                Ok(Response::new(Empty {}))
            }
            Err(e) => {
                self.record("grpc", "Acknowledge", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn modify_ack_deadline(
        &self,
        request: Request<ModifyAckDeadlineRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.authorize(&request, &request.get_ref().subscription)
            .await?;
        let req = request.into_inner();
        if !(0..=600).contains(&req.ack_deadline_seconds) {
            return Err(Status::invalid_argument(
                "ack_deadline_seconds must be between 0 and 600",
            ));
        }
        match self
            .backend
            .modify_ack_deadline(
                &req.subscription,
                &req.ack_ids,
                req.ack_deadline_seconds as u32,
            )
            .await
        {
            Ok(()) => {
                self.record("grpc", "ModifyAckDeadline", "ok");
                Ok(Response::new(Empty {}))
            }
            Err(e) => {
                self.record("grpc", "ModifyAckDeadline", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn seek(&self, request: Request<SeekRequest>) -> Result<Response<SeekResponse>, Status> {
        self.authorize(&request, &request.get_ref().subscription)
            .await?;
        let req = request.into_inner();
        match req.target {
            Some(seek_request::Target::Time(ts)) => {
                self.backend
                    .seek_to_time(&req.subscription, timestamp_to_datetime(ts)?)
                    .await
                    .map_err(backend_status)?;
                self.record("grpc", "Seek", "ok");
                Ok(Response::new(SeekResponse {}))
            }
            Some(seek_request::Target::Snapshot(snapshot)) => {
                self.backend
                    .seek_to_snapshot(&req.subscription, &snapshot)
                    .await
                    .map_err(backend_status)?;
                self.record("grpc", "Seek", "ok");
                Ok(Response::new(SeekResponse {}))
            }
            None => Err(Status::invalid_argument("seek target is required")),
        }
    }

    async fn create_snapshot(
        &self,
        request: Request<CreateSnapshotRequest>,
    ) -> Result<Response<Snapshot>, Status> {
        self.authorize(&request, &request.get_ref().name).await?;
        let req = request.into_inner();
        match self
            .backend
            .create_snapshot(&req.name, &req.subscription, req.labels)
            .await
        {
            Ok(snapshot) => {
                self.record("grpc", "CreateSnapshot", "ok");
                Ok(Response::new(snapshot_to_proto(snapshot)))
            }
            Err(e) => {
                self.record("grpc", "CreateSnapshot", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn update_snapshot(
        &self,
        request: Request<UpdateSnapshotRequest>,
    ) -> Result<Response<Snapshot>, Status> {
        let req = request.get_ref();
        let snapshot = req
            .snapshot
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("snapshot is required"))?;
        self.authorize(&request, &snapshot.name).await?;
        let spec = snapshot_from_proto(snapshot.clone())?;
        let update_mask = field_mask_paths(req.update_mask.clone());
        match self.backend.update_snapshot(spec, &update_mask).await {
            Ok(snapshot) => {
                self.record("grpc", "UpdateSnapshot", "ok");
                Ok(Response::new(snapshot_to_proto(snapshot)))
            }
            Err(e) => {
                self.record("grpc", "UpdateSnapshot", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn get_snapshot(
        &self,
        request: Request<GetSnapshotRequest>,
    ) -> Result<Response<Snapshot>, Status> {
        self.authorize(&request, &request.get_ref().snapshot)
            .await?;
        match self
            .backend
            .get_snapshot(&request.into_inner().snapshot)
            .await
        {
            Ok(snapshot) => {
                self.record("grpc", "GetSnapshot", "ok");
                Ok(Response::new(snapshot_to_proto(snapshot)))
            }
            Err(e) => {
                self.record("grpc", "GetSnapshot", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn list_snapshots(
        &self,
        request: Request<ListSnapshotsRequest>,
    ) -> Result<Response<ListSnapshotsResponse>, Status> {
        self.authorize(&request, &request.get_ref().project).await?;
        let req = request.into_inner();
        match self
            .backend
            .list_snapshots(&req.project, req.page_size, &req.page_token)
            .await
        {
            Ok(page) => {
                self.record("grpc", "ListSnapshots", "ok");
                Ok(Response::new(ListSnapshotsResponse {
                    snapshots: page.items.into_iter().map(snapshot_to_proto).collect(),
                    next_page_token: page.next_page_token,
                }))
            }
            Err(e) => {
                self.record("grpc", "ListSnapshots", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn delete_snapshot(
        &self,
        request: Request<DeleteSnapshotRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.authorize(&request, &request.get_ref().snapshot)
            .await?;
        match self
            .backend
            .delete_snapshot(&request.into_inner().snapshot)
            .await
        {
            Ok(()) => {
                self.record("grpc", "DeleteSnapshot", "ok");
                Ok(Response::new(Empty {}))
            }
            Err(e) => {
                self.record("grpc", "DeleteSnapshot", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn get_iam_policy(
        &self,
        request: Request<GetIamPolicyRequest>,
    ) -> Result<Response<Policy>, Status> {
        self.authorize(&request, &request.get_ref().resource)
            .await?;
        match self
            .backend
            .get_iam_policy(&request.into_inner().resource)
            .await
        {
            Ok(policy) => {
                self.record("grpc", "GetIamPolicy", "ok");
                Ok(Response::new(policy_to_proto(policy)))
            }
            Err(e) => {
                self.record("grpc", "GetIamPolicy", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn set_iam_policy(
        &self,
        request: Request<SetIamPolicyRequest>,
    ) -> Result<Response<Policy>, Status> {
        self.authorize(&request, &request.get_ref().resource)
            .await?;
        let req = request.into_inner();
        let policy = req
            .policy
            .ok_or_else(|| Status::invalid_argument("policy is required"))?;
        match self
            .backend
            .set_iam_policy(&req.resource, policy_from_proto(policy))
            .await
        {
            Ok(updated) => {
                self.record("grpc", "SetIamPolicy", "ok");
                Ok(Response::new(policy_to_proto(updated)))
            }
            Err(e) => {
                self.record("grpc", "SetIamPolicy", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn test_iam_permissions(
        &self,
        request: Request<TestIamPermissionsRequest>,
    ) -> Result<Response<TestIamPermissionsResponse>, Status> {
        self.authorize(&request, &request.get_ref().resource)
            .await?;
        let req = request.into_inner();
        match self
            .backend
            .test_iam_permissions(&req.resource, &req.permissions)
            .await
        {
            Ok(granted) => {
                self.record("grpc", "TestIamPermissions", "ok");
                Ok(Response::new(TestIamPermissionsResponse {
                    permissions: granted,
                }))
            }
            Err(e) => {
                self.record("grpc", "TestIamPermissions", "error");
                Err(backend_status(e))
            }
        }
    }
}

#[tonic::async_trait]
impl SchemaService for GatewayService {
    async fn create_schema(
        &self,
        request: Request<CreateSchemaRequest>,
    ) -> Result<Response<Schema>, Status> {
        self.authorize(&request, &request.get_ref().parent).await?;
        let req = request.into_inner();
        let mut schema = req
            .schema
            .ok_or_else(|| Status::invalid_argument("schema is required"))?;
        if schema.name.is_empty() {
            if req.schema_id.is_empty() {
                return Err(Status::invalid_argument(
                    "schema_id is required when schema.name is empty",
                ));
            }
            schema.name = format!(
                "{}/schemas/{}",
                req.parent.trim_end_matches('/'),
                req.schema_id
            );
        }
        match self.backend.create_schema(schema_from_proto(schema)?).await {
            Ok(created) => {
                self.record("grpc", "CreateSchema", "ok");
                Ok(Response::new(schema_to_proto(created)))
            }
            Err(e) => {
                self.record("grpc", "CreateSchema", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn get_schema(
        &self,
        request: Request<GetSchemaRequest>,
    ) -> Result<Response<Schema>, Status> {
        self.authorize(&request, &request.get_ref().name).await?;
        match self.backend.get_schema(&request.into_inner().name).await {
            Ok(schema) => {
                self.record("grpc", "GetSchema", "ok");
                Ok(Response::new(schema_to_proto(schema)))
            }
            Err(e) => {
                self.record("grpc", "GetSchema", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn list_schemas(
        &self,
        request: Request<ListSchemasRequest>,
    ) -> Result<Response<ListSchemasResponse>, Status> {
        self.authorize(&request, &request.get_ref().parent).await?;
        let req = request.into_inner();
        match self
            .backend
            .list_schemas(&req.parent, req.page_size, &req.page_token)
            .await
        {
            Ok(page) => {
                self.record("grpc", "ListSchemas", "ok");
                Ok(Response::new(ListSchemasResponse {
                    schemas: page.items.into_iter().map(schema_to_proto).collect(),
                    next_page_token: page.next_page_token,
                }))
            }
            Err(e) => {
                self.record("grpc", "ListSchemas", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn delete_schema(
        &self,
        request: Request<DeleteSchemaRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.authorize(&request, &request.get_ref().name).await?;
        match self.backend.delete_schema(&request.into_inner().name).await {
            Ok(()) => {
                self.record("grpc", "DeleteSchema", "ok");
                Ok(Response::new(Empty {}))
            }
            Err(e) => {
                self.record("grpc", "DeleteSchema", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn validate_schema(
        &self,
        request: Request<ValidateSchemaRequest>,
    ) -> Result<Response<ValidateSchemaResponse>, Status> {
        self.authorize(&request, &request.get_ref().parent).await?;
        let req = request.into_inner();
        let schema = req
            .schema
            .ok_or_else(|| Status::invalid_argument("schema is required"))?;
        let spec = schema_from_proto(schema)?;
        match self.backend.validate_schema(&spec).await {
            Ok(()) => {
                self.record("grpc", "ValidateSchema", "ok");
                Ok(Response::new(ValidateSchemaResponse {}))
            }
            Err(e) => {
                self.record("grpc", "ValidateSchema", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn validate_message(
        &self,
        request: Request<ValidateMessageRequest>,
    ) -> Result<Response<ValidateMessageResponse>, Status> {
        self.authorize(&request, &request.get_ref().parent).await?;
        let req = request.into_inner();
        let result = match req.schema_spec {
            Some(ValidateMessageSchemaSpec::Name(name)) => {
                self.backend
                    .validate_message(Some(&name), None, &req.message)
                    .await
            }
            Some(ValidateMessageSchemaSpec::Schema(schema)) => {
                let spec = schema_from_proto(schema)?;
                self.backend
                    .validate_message(None, Some(&spec), &req.message)
                    .await
            }
            None => {
                return Err(Status::invalid_argument("schema_spec is required"));
            }
        };
        match result {
            Ok(()) => {
                self.record("grpc", "ValidateMessage", "ok");
                Ok(Response::new(ValidateMessageResponse {}))
            }
            Err(e) => {
                self.record("grpc", "ValidateMessage", "error");
                Err(backend_status(e))
            }
        }
    }
}
