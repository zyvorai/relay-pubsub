// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

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
    #[serde(default)]
    pub push_attributes: HashMap<String, String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotSpec {
    pub name: String,
    pub topic: String,
    pub expire_time: DateTime<Utc>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// Topic message index captured at snapshot creation (seek restores to this).
    pub topic_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaSpec {
    pub name: String,
    /// "PROTOCOL_BUFFER" | "AVRO" | "UNSPECIFIED"
    pub schema_type: String,
    pub definition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IamBinding {
    pub role: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IamPolicy {
    pub version: i32,
    pub bindings: Vec<IamBinding>,
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_page_token: String,
}

/// Non-consuming peek of a stored message (admin / console).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessagePreview {
    pub id: String,
    pub data: String,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    #[serde(default)]
    pub ordering_key: String,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopicInventory {
    pub name: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    pub message_count: u64,
    pub recent: Vec<MessagePreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionInventory {
    pub name: String,
    pub topic: String,
    pub ack_deadline_seconds: u32,
    pub enable_message_ordering: bool,
    pub enable_exactly_once_delivery: bool,
    #[serde(default)]
    pub push_endpoint: Option<String>,
    #[serde(default)]
    pub push_attributes: HashMap<String, String>,
    #[serde(default)]
    pub dead_letter_topic: Option<String>,
    /// Messages retained on the topic.
    pub topic_message_count: u64,
    /// Cursor into the topic stream (next index to deliver).
    pub next_index: u64,
    /// Unacked / not-yet-pulled estimate.
    pub backlog: u64,
    /// Currently leased (inflight) deliveries.
    pub inflight: u64,
    /// Messages waiting on retry backoff.
    pub retry_queued: u64,
    /// Distinct indices acknowledged.
    pub acked: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct InventoryReport {
    pub topics: Vec<TopicInventory>,
    pub subscriptions: Vec<SubscriptionInventory>,
}

pub fn paginate<T: Clone>(items: Vec<T>, page_size: i32, page_token: &str) -> Page<T> {
    let size = if page_size <= 0 {
        100
    } else {
        page_size.clamp(1, 1000) as usize
    };
    let start = if page_token.is_empty() {
        0
    } else {
        page_token.parse::<usize>().unwrap_or(0)
    };
    let end = (start + size).min(items.len());
    let slice = items.get(start..end).unwrap_or(&[]).to_vec();
    let next = if end < items.len() {
        end.to_string()
    } else {
        String::new()
    };
    Page {
        items: slice,
        next_page_token: next,
    }
}
