// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

use crate::model::{Delivery, NewMessage, SubscriptionSpec, TopicSpec};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
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
}

#[async_trait]
pub trait RelayBackend: Send + Sync + 'static {
    async fn create_topic(&self, topic: TopicSpec) -> Result<TopicSpec, BackendError>;
    async fn get_topic(&self, name: &str) -> Result<TopicSpec, BackendError>;
    async fn list_topics(&self, project: &str) -> Result<Vec<TopicSpec>, BackendError>;
    async fn delete_topic(&self, name: &str) -> Result<(), BackendError>;

    async fn create_subscription(
        &self,
        subscription: SubscriptionSpec,
    ) -> Result<SubscriptionSpec, BackendError>;
    async fn get_subscription(&self, name: &str) -> Result<SubscriptionSpec, BackendError>;
    async fn list_subscriptions(
        &self,
        project: &str,
    ) -> Result<Vec<SubscriptionSpec>, BackendError>;
    async fn delete_subscription(&self, name: &str) -> Result<(), BackendError>;

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
}
