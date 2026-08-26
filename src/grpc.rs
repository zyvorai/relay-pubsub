// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

use crate::backend::{BackendError, RelayBackend};
use crate::google::pubsub::v1::{
    publisher_server::Publisher, seek_request, subscriber_server::Subscriber, AcknowledgeRequest,
    DeleteSubscriptionRequest, DeleteTopicRequest, Empty, GetSubscriptionRequest, GetTopicRequest,
    ListSubscriptionsRequest, ListSubscriptionsResponse, ListTopicsRequest, ListTopicsResponse,
    ModifyAckDeadlineRequest, PublishRequest, PublishResponse, PubsubMessage, PullRequest,
    PullResponse, ReceivedMessage, SeekRequest, SeekResponse, StreamingPullRequest,
    StreamingPullResponse, Subscription, Topic,
};
use crate::metrics::Metrics;
use crate::model::{DeadLetterSpec, Delivery, NewMessage, RetrySpec, SubscriptionSpec, TopicSpec};
use chrono::{DateTime, Utc};
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct GatewayService {
    backend: Arc<dyn RelayBackend>,
    auth_token: Option<String>,
    metrics: Metrics,
}

impl GatewayService {
    pub fn new(
        backend: Arc<dyn RelayBackend>,
        auth_token: Option<String>,
        metrics: Metrics,
    ) -> Self {
        Self {
            backend,
            auth_token,
            metrics,
        }
    }

    fn authorize<T>(&self, request: &Request<T>) -> Result<(), Status> {
        let Some(expected) = &self.auth_token else {
            return Ok(());
        };
        let value = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        let expected_header = format!("Bearer {expected}");
        if value == Some(expected_header.as_str()) {
            Ok(())
        } else {
            Err(Status::unauthenticated("missing or invalid bearer token"))
        }
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
        BackendError::Unavailable(v) => Status::unavailable(v),
        BackendError::Internal(v) => Status::internal(v),
    }
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
    Subscription {
        name: value.name,
        topic: value.topic,
        push_config: value
            .push_endpoint
            .map(|endpoint| crate::google::pubsub::v1::PushConfig {
                push_endpoint: endpoint,
                attributes: Default::default(),
            }),
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
        push_endpoint: value.push_config.and_then(|v| {
            if v.push_endpoint.is_empty() {
                None
            } else {
                Some(v.push_endpoint)
            }
        }),
    })
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
        .ok_or_else(|| Status::invalid_argument("invalid seek timestamp"))
}

#[tonic::async_trait]
impl Publisher for GatewayService {
    async fn create_topic(&self, request: Request<Topic>) -> Result<Response<Topic>, Status> {
        self.authorize(&request)?;
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

    async fn get_topic(
        &self,
        request: Request<GetTopicRequest>,
    ) -> Result<Response<Topic>, Status> {
        self.authorize(&request)?;
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
        self.authorize(&request)?;
        let request = request.into_inner();
        let result = self.backend.list_topics(&request.project).await;
        match result {
            Ok(mut topics) => {
                let limit = if request.page_size <= 0 {
                    100
                } else {
                    request.page_size.min(1000) as usize
                };
                topics.truncate(limit);
                self.record("grpc", "ListTopics", "ok");
                Ok(Response::new(ListTopicsResponse {
                    topics: topics.into_iter().map(topic_to_proto).collect(),
                    next_page_token: String::new(),
                }))
            }
            Err(e) => {
                self.record("grpc", "ListTopics", "error");
                Err(backend_status(e))
            }
        }
    }

    async fn delete_topic(
        &self,
        request: Request<DeleteTopicRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.authorize(&request)?;
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
        self.authorize(&request)?;
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
}

#[tonic::async_trait]
impl Subscriber for GatewayService {
    async fn create_subscription(
        &self,
        request: Request<Subscription>,
    ) -> Result<Response<Subscription>, Status> {
        self.authorize(&request)?;
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

    async fn get_subscription(
        &self,
        request: Request<GetSubscriptionRequest>,
    ) -> Result<Response<Subscription>, Status> {
        self.authorize(&request)?;
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
        self.authorize(&request)?;
        let request = request.into_inner();
        match self.backend.list_subscriptions(&request.project).await {
            Ok(mut subscriptions) => {
                let limit = if request.page_size <= 0 {
                    100
                } else {
                    request.page_size.min(1000) as usize
                };
                subscriptions.truncate(limit);
                self.record("grpc", "ListSubscriptions", "ok");
                Ok(Response::new(ListSubscriptionsResponse {
                    subscriptions: subscriptions
                        .into_iter()
                        .map(subscription_to_proto)
                        .collect(),
                    next_page_token: String::new(),
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
        self.authorize(&request)?;
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

    async fn pull(&self, request: Request<PullRequest>) -> Result<Response<PullResponse>, Status> {
        self.authorize(&request)?;
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
        self.authorize(&request)?;
        let mut input = request.into_inner();
        let backend = self.backend.clone();
        let metrics = self.metrics.clone();
        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            let mut subscription = String::new();
            let mut batch_size = 100u32;
            let mut interval = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    incoming = input.message() => {
                        match incoming {
                            Ok(Some(frame)) => {
                                if !frame.subscription.is_empty() { subscription = frame.subscription.clone(); }
                                if frame.max_outstanding_messages > 0 {
                                    batch_size = frame.max_outstanding_messages.min(1000) as u32;
                                }
                                if subscription.is_empty() { continue; }
                                if !frame.ack_ids.is_empty() {
                                    if let Err(e) = backend.acknowledge(&subscription, &frame.ack_ids).await {
                                        let _ = tx.send(Err(backend_status(e))).await; break;
                                    }
                                }
                                for (ack_id, deadline) in frame.modify_deadline_ack_ids.iter().zip(frame.modify_deadline_seconds.iter()) {
                                    if let Err(e) = backend.modify_ack_deadline(&subscription, std::slice::from_ref(ack_id), (*deadline).max(0) as u32).await {
                                        let _ = tx.send(Err(backend_status(e))).await; return;
                                    }
                                }
                            }
                            Ok(None) => break,
                            Err(status) => { let _ = tx.send(Err(status)).await; break; }
                        }
                    }
                    _ = interval.tick(), if !subscription.is_empty() => {
                        match backend.pull(&subscription, batch_size).await {
                            Ok(deliveries) if !deliveries.is_empty() => {
                                metrics.messages.with_label_values(&["delivered"]).inc_by(deliveries.len() as u64);
                                let response = StreamingPullResponse { received_messages: deliveries.into_iter().map(delivery_to_proto).collect() };
                                if tx.send(Ok(response)).await.is_err() { break; }
                            }
                            Ok(_) => {}
                            Err(e) => { let _ = tx.send(Err(backend_status(e))).await; break; }
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
        self.authorize(&request)?;
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
        self.authorize(&request)?;
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
        self.authorize(&request)?;
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
            Some(seek_request::Target::Snapshot(_)) => Err(Status::unimplemented(
                "snapshot seek is not implemented in v0.1; time-based replay is supported",
            )),
            None => Err(Status::invalid_argument("seek target is required")),
        }
    }
}
