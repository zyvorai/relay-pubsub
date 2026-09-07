// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

use crate::model::{
    Delivery, IamPolicy, InventoryReport, MessagePreview, NewMessage, Page, SchemaSpec,
    SnapshotSpec, SubscriptionInventory, TopicInventory, SubscriptionSpec, TopicSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("resource already exists: {0}")]
    AlreadyExists(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("failed precondition: {0}")]
    FailedPrecondition(String),
    #[error("backend unavailable: {0}")]
    Unavailable(String),
    #[error("internal backend error: {0}")]
    Internal(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
}

#[async_trait]
pub trait RelayBackend: Send + Sync + 'static {
    async fn create_topic(&self, topic: TopicSpec) -> Result<TopicSpec, BackendError>;
    async fn update_topic(
        &self,
        topic: TopicSpec,
        update_mask: &[String],
    ) -> Result<TopicSpec, BackendError>;
    async fn get_topic(&self, name: &str) -> Result<TopicSpec, BackendError>;
    async fn list_topics(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<TopicSpec>, BackendError>;
    async fn list_topic_subscriptions(
        &self,
        topic: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<String>, BackendError>;
    async fn delete_topic(&self, name: &str) -> Result<(), BackendError>;

    async fn create_subscription(
        &self,
        subscription: SubscriptionSpec,
    ) -> Result<SubscriptionSpec, BackendError>;
    async fn update_subscription(
        &self,
        subscription: SubscriptionSpec,
        update_mask: &[String],
    ) -> Result<SubscriptionSpec, BackendError>;
    async fn get_subscription(&self, name: &str) -> Result<SubscriptionSpec, BackendError>;
    async fn list_subscriptions(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SubscriptionSpec>, BackendError>;
    async fn delete_subscription(&self, name: &str) -> Result<(), BackendError>;
    async fn modify_push_config(
        &self,
        subscription: &str,
        push_endpoint: Option<String>,
        push_attributes: HashMap<String, String>,
    ) -> Result<(), BackendError>;

    async fn publish(
        &self,
        topic: &str,
        messages: Vec<NewMessage>,
    ) -> Result<Vec<String>, BackendError>;
    async fn pull(
        &self,
        subscription: &str,
        max_messages: u32,
    ) -> Result<Vec<Delivery>, BackendError>;
    async fn acknowledge(&self, subscription: &str, ack_ids: &[String])
        -> Result<(), BackendError>;
    async fn modify_ack_deadline(
        &self,
        subscription: &str,
        ack_ids: &[String],
        seconds: u32,
    ) -> Result<(), BackendError>;
    async fn seek_to_time(
        &self,
        subscription: &str,
        time: DateTime<Utc>,
    ) -> Result<(), BackendError>;
    async fn seek_to_snapshot(
        &self,
        subscription: &str,
        snapshot: &str,
    ) -> Result<(), BackendError>;

    async fn create_snapshot(
        &self,
        name: &str,
        subscription: &str,
        labels: std::collections::HashMap<String, String>,
    ) -> Result<SnapshotSpec, BackendError>;
    async fn update_snapshot(
        &self,
        snapshot: SnapshotSpec,
        update_mask: &[String],
    ) -> Result<SnapshotSpec, BackendError>;
    async fn get_snapshot(&self, name: &str) -> Result<SnapshotSpec, BackendError>;
    async fn list_snapshots(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SnapshotSpec>, BackendError>;
    async fn delete_snapshot(&self, name: &str) -> Result<(), BackendError>;

    async fn get_iam_policy(&self, resource: &str) -> Result<IamPolicy, BackendError>;
    async fn set_iam_policy(
        &self,
        resource: &str,
        policy: IamPolicy,
    ) -> Result<IamPolicy, BackendError>;
    async fn test_iam_permissions(
        &self,
        resource: &str,
        permissions: &[String],
    ) -> Result<Vec<String>, BackendError>;

    async fn create_schema(&self, schema: SchemaSpec) -> Result<SchemaSpec, BackendError>;
    async fn get_schema(&self, name: &str) -> Result<SchemaSpec, BackendError>;
    async fn list_schemas(
        &self,
        parent: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SchemaSpec>, BackendError>;
    async fn delete_schema(&self, name: &str) -> Result<(), BackendError>;
    async fn validate_schema(&self, schema: &SchemaSpec) -> Result<(), BackendError>;
    async fn validate_message(
        &self,
        schema_name: Option<&str>,
        schema: Option<&SchemaSpec>,
        message: &[u8],
    ) -> Result<(), BackendError>;

    /// Subscriptions with a push endpoint configured (for the push dispatcher).
    async fn list_push_subscriptions(&self) -> Result<Vec<SubscriptionSpec>, BackendError>;

    /// Non-consuming inventory for the ops console (incoming / outgoing / stored).
    async fn inventory(&self, project: &str) -> Result<InventoryReport, BackendError> {
        let topics = self.list_topics(project, 1000, "").await?;
        let subscriptions = self.list_subscriptions(project, 1000, "").await?;
        Ok(InventoryReport {
            topics: topics
                .items
                .into_iter()
                .map(|t| TopicInventory {
                    name: t.name,
                    labels: t.labels,
                    message_count: 0,
                    recent: Vec::<MessagePreview>::new(),
                })
                .collect(),
            subscriptions: subscriptions
                .items
                .into_iter()
                .map(|s| SubscriptionInventory {
                    name: s.name,
                    topic: s.topic,
                    ack_deadline_seconds: s.ack_deadline_seconds,
                    enable_message_ordering: s.enable_message_ordering,
                    enable_exactly_once_delivery: s.enable_exactly_once_delivery,
                    push_endpoint: s.push_endpoint,
                    push_attributes: s.push_attributes,
                    dead_letter_topic: s.dead_letter.map(|d| d.topic),
                    filter: s.filter,
                    topic_message_count: 0,
                    next_index: 0,
                    backlog: 0,
                    inflight: 0,
                    retry_queued: 0,
                    acked: 0,
                })
                .collect(),
        })
    }
}
