use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopicSpec {
    pub name: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub kms_key_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeadLetterSpec {
    pub topic: String,
    pub max_delivery_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrySpec {
    pub minimum_backoff_seconds: u32,
    pub maximum_backoff_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionSpec {
    pub name: String,
    pub topic: String,
    pub ack_deadline_seconds: u32,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub enable_message_ordering: bool,
    #[serde(default)]
    pub enable_exactly_once_delivery: bool,
    #[serde(default)]
    pub dead_letter: Option<DeadLetterSpec>,
    #[serde(default)]
    pub retry: Option<RetrySpec>,
    #[serde(default)]
    pub push_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayMessage {
    pub id: String,
    pub data: Vec<u8>,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    #[serde(default)]
    pub ordering_key: String,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewMessage {
    pub data: Vec<u8>,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    #[serde(default)]
    pub ordering_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Delivery {
    pub ack_id: String,
    pub message: RelayMessage,
    pub delivery_attempt: u32,
}
