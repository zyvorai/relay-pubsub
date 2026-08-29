// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

use crate::backend::{BackendError, RelayBackend};
use crate::model::{
    paginate, Delivery, IamBinding, IamPolicy, InventoryReport, MessagePreview, NewMessage, Page,
    RelayMessage, SchemaSpec, SnapshotSpec, SubscriptionInventory, SubscriptionSpec,
    TopicInventory, TopicSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub struct MemoryBackend {
    state: RwLock<State>,
    persist_path: Option<PathBuf>,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    topics: HashMap<String, TopicState>,
    subscriptions: HashMap<String, SubscriptionState>,
    snapshots: HashMap<String, SnapshotSpec>,
    schemas: HashMap<String, SchemaSpec>,
    iam: HashMap<String, IamPolicy>,
}

#[derive(Serialize, Deserialize)]
struct TopicState {
    spec: TopicSpec,
    messages: Vec<RelayMessage>,
}

#[derive(Serialize, Deserialize)]
struct SubscriptionState {
    spec: SubscriptionSpec,
    next_index: usize,
    acked: HashSet<usize>,
    inflight: HashMap<String, Inflight>,
    retry_queue: VecDeque<RetryEntry>,
    attempts: HashMap<usize, u32>,
    /// Last acked topic index per ordering key (for ordered delivery).
    ordering_acked: HashMap<String, usize>,
}

#[derive(Serialize, Deserialize)]
struct Inflight {
    topic_index: usize,
    deadline: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
struct RetryEntry {
    topic_index: usize,
    available_at: DateTime<Utc>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(State::default()),
            persist_path: None,
        }
    }

    /// Load-or-create a durable store at `path` (JSON snapshot after each mutation).
    pub fn with_persistence(path: impl AsRef<Path>) -> Result<Self, BackendError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| BackendError::Internal(format!("create data dir: {e}")))?;
        }
        let state = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| BackendError::Internal(format!("read state: {e}")))?;
            serde_json::from_str(&raw)
                .map_err(|e| BackendError::Internal(format!("parse state: {e}")))?
        } else {
            State::default()
        };
        Ok(Self {
            state: RwLock::new(state),
            persist_path: Some(path),
        })
    }

    fn belongs_to_project(resource: &str, project: &str) -> bool {
        resource.starts_with(&format!("{project}/"))
    }

    fn normalize_ack_deadline(seconds: u32) -> u32 {
        seconds.clamp(10, 600)
    }

    fn persist_locked(&self, state: &State) -> Result<(), BackendError> {
        let Some(path) = &self.persist_path else {
            return Ok(());
        };
        let tmp = path.with_extension("json.tmp");
        let raw = serde_json::to_string(state)
            .map_err(|e| BackendError::Internal(format!("serialize state: {e}")))?;
        std::fs::write(&tmp, raw)
            .map_err(|e| BackendError::Internal(format!("write state tmp: {e}")))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| BackendError::Internal(format!("rename state: {e}")))?;
        Ok(())
    }

    fn backoff_seconds(spec: &SubscriptionSpec, attempt: u32) -> u32 {
        let (min_b, max_b) = match &spec.retry {
            Some(r) => (
                r.minimum_backoff_seconds,
                r.maximum_backoff_seconds.max(r.minimum_backoff_seconds),
            ),
            None => (10, 600),
        };
        if max_b == 0 {
            return 0;
        }
        let exp = attempt.saturating_sub(1).min(10);
        let delay = min_b.saturating_mul(1u32 << exp.min(16));
        delay.clamp(min_b, max_b.max(1))
    }

    fn apply_topic_mask(existing: &mut TopicSpec, incoming: &TopicSpec, mask: &[String]) {
        if mask.is_empty() || mask.iter().any(|f| f == "*") {
            existing.labels = incoming.labels.clone();
            existing.kms_key_name = incoming.kms_key_name.clone();
            return;
        }
        for field in mask {
            match field.as_str() {
                "labels" => existing.labels = incoming.labels.clone(),
                "kms_key_name" | "kmsKeyName" => {
                    existing.kms_key_name = incoming.kms_key_name.clone()
                }
                _ => {}
            }
        }
    }

    fn apply_subscription_mask(
        existing: &mut SubscriptionSpec,
        incoming: &SubscriptionSpec,
        mask: &[String],
    ) {
        if mask.is_empty() || mask.iter().any(|f| f == "*") {
            existing.ack_deadline_seconds = incoming.ack_deadline_seconds;
            existing.labels = incoming.labels.clone();
            existing.enable_message_ordering = incoming.enable_message_ordering;
            existing.enable_exactly_once_delivery = incoming.enable_exactly_once_delivery;
            existing.dead_letter = incoming.dead_letter.clone();
            existing.retry = incoming.retry.clone();
            existing.push_endpoint = incoming.push_endpoint.clone();
            existing.push_attributes = incoming.push_attributes.clone();
            return;
        }
        for field in mask {
            match field.as_str() {
                "ack_deadline_seconds" | "ackDeadlineSeconds" => {
                    existing.ack_deadline_seconds = incoming.ack_deadline_seconds
                }
                "labels" => existing.labels = incoming.labels.clone(),
                "enable_message_ordering" | "enableMessageOrdering" => {
                    existing.enable_message_ordering = incoming.enable_message_ordering
                }
                "enable_exactly_once_delivery" | "enableExactlyOnceDelivery" => {
                    existing.enable_exactly_once_delivery = incoming.enable_exactly_once_delivery
                }
                "dead_letter_policy" | "deadLetterPolicy" => {
                    existing.dead_letter = incoming.dead_letter.clone()
                }
                "retry_policy" | "retryPolicy" => existing.retry = incoming.retry.clone(),
                "push_config" | "pushConfig" => {
                    existing.push_endpoint = incoming.push_endpoint.clone();
                    existing.push_attributes = incoming.push_attributes.clone();
                }
                _ => {}
            }
        }
    }

    fn ordering_blocked(
        sub: &SubscriptionState,
        topic: &TopicState,
        index: usize,
        now: DateTime<Utc>,
    ) -> bool {
        if !sub.spec.enable_message_ordering {
            return false;
        }
        let key = &topic.messages[index].ordering_key;
        if key.is_empty() {
            return false;
        }
        // Block if any earlier unacked message with the same key exists.
        for (i, msg) in topic.messages.iter().enumerate().take(index) {
            if msg.ordering_key != *key {
                continue;
            }
            if sub.acked.contains(&i) {
                continue;
            }
            if sub
                .inflight
                .values()
                .any(|v| v.topic_index == i && v.deadline > now)
            {
                return true;
            }
            if !sub.acked.contains(&i) {
                return true;
            }
        }
        false
    }
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared handle used by push dispatcher and RelayEventsBackend.
pub type SharedMemory = Arc<MemoryBackend>;

#[async_trait]
impl RelayBackend for MemoryBackend {
    async fn create_topic(&self, topic: TopicSpec) -> Result<TopicSpec, BackendError> {
        if !topic.name.starts_with("projects/") || !topic.name.contains("/topics/") {
            return Err(BackendError::InvalidArgument(format!(
                "invalid topic name {}",
                topic.name
            )));
        }
        let mut state = self.state.write().await;
        if state.topics.contains_key(&topic.name) {
            return Err(BackendError::AlreadyExists(topic.name));
        }
        state.topics.insert(
            topic.name.clone(),
            TopicState {
                spec: topic.clone(),
                messages: Vec::new(),
            },
        );
        self.persist_locked(&state)?;
        Ok(topic)
    }

    async fn update_topic(
        &self,
        topic: TopicSpec,
        update_mask: &[String],
    ) -> Result<TopicSpec, BackendError> {
        let mut state = self.state.write().await;
        let entry = state
            .topics
            .get_mut(&topic.name)
            .ok_or_else(|| BackendError::NotFound(topic.name.clone()))?;
        Self::apply_topic_mask(&mut entry.spec, &topic, update_mask);
        let out = entry.spec.clone();
        self.persist_locked(&state)?;
        Ok(out)
    }

    async fn get_topic(&self, name: &str) -> Result<TopicSpec, BackendError> {
        let state = self.state.read().await;
        state
            .topics
            .get(name)
            .map(|t| t.spec.clone())
            .ok_or_else(|| BackendError::NotFound(name.to_string()))
    }

    async fn list_topics(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<TopicSpec>, BackendError> {
        let state = self.state.read().await;
        let mut topics: Vec<_> = state
            .topics
            .values()
            .filter(|t| Self::belongs_to_project(&t.spec.name, project))
            .map(|t| t.spec.clone())
            .collect();
        topics.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(paginate(topics, page_size, page_token))
    }

    async fn list_topic_subscriptions(
        &self,
        topic: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<String>, BackendError> {
        let state = self.state.read().await;
        if !state.topics.contains_key(topic) {
            return Err(BackendError::NotFound(topic.to_string()));
        }
        let mut names: Vec<_> = state
            .subscriptions
            .values()
            .filter(|s| s.spec.topic == topic)
            .map(|s| s.spec.name.clone())
            .collect();
        names.sort();
        Ok(paginate(names, page_size, page_token))
    }

    async fn delete_topic(&self, name: &str) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        if state.topics.remove(name).is_none() {
            return Err(BackendError::NotFound(name.to_string()));
        }
        self.persist_locked(&state)?;
        Ok(())
    }

    async fn create_subscription(
        &self,
        mut subscription: SubscriptionSpec,
    ) -> Result<SubscriptionSpec, BackendError> {
        if !subscription.name.starts_with("projects/")
            || !subscription.name.contains("/subscriptions/")
        {
            return Err(BackendError::InvalidArgument(format!(
                "invalid subscription name {}",
                subscription.name
            )));
        }
        let mut state = self.state.write().await;
        if state.subscriptions.contains_key(&subscription.name) {
            return Err(BackendError::AlreadyExists(subscription.name));
        }
        let topic_len = state
            .topics
            .get(&subscription.topic)
            .ok_or_else(|| BackendError::NotFound(subscription.topic.clone()))?
            .messages
            .len();
        subscription.ack_deadline_seconds =
            Self::normalize_ack_deadline(subscription.ack_deadline_seconds.max(10));
        let spec = subscription.clone();
        state.subscriptions.insert(
            subscription.name.clone(),
            SubscriptionState {
                spec: subscription,
                next_index: topic_len,
                acked: HashSet::new(),
                inflight: HashMap::new(),
                retry_queue: VecDeque::new(),
                attempts: HashMap::new(),
                ordering_acked: HashMap::new(),
            },
        );
        self.persist_locked(&state)?;
        Ok(spec)
    }

    async fn update_subscription(
        &self,
        subscription: SubscriptionSpec,
        update_mask: &[String],
    ) -> Result<SubscriptionSpec, BackendError> {
        let mut state = self.state.write().await;
        let entry = state
            .subscriptions
            .get_mut(&subscription.name)
            .ok_or_else(|| BackendError::NotFound(subscription.name.clone()))?;
        Self::apply_subscription_mask(&mut entry.spec, &subscription, update_mask);
        entry.spec.ack_deadline_seconds =
            Self::normalize_ack_deadline(entry.spec.ack_deadline_seconds.max(10));
        let out = entry.spec.clone();
        self.persist_locked(&state)?;
        Ok(out)
    }

    async fn get_subscription(&self, name: &str) -> Result<SubscriptionSpec, BackendError> {
        let state = self.state.read().await;
        state
            .subscriptions
            .get(name)
            .map(|s| s.spec.clone())
            .ok_or_else(|| BackendError::NotFound(name.to_string()))
    }

    async fn list_subscriptions(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SubscriptionSpec>, BackendError> {
        let state = self.state.read().await;
        let mut subscriptions: Vec<_> = state
            .subscriptions
            .values()
            .filter(|s| Self::belongs_to_project(&s.spec.name, project))
            .map(|s| s.spec.clone())
            .collect();
        subscriptions.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(paginate(subscriptions, page_size, page_token))
    }

    async fn delete_subscription(&self, name: &str) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        if state.subscriptions.remove(name).is_none() {
            return Err(BackendError::NotFound(name.to_string()));
        }
        self.persist_locked(&state)?;
        Ok(())
    }

    async fn modify_push_config(
        &self,
        subscription: &str,
        push_endpoint: Option<String>,
        push_attributes: HashMap<String, String>,
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        let sub = state
            .subscriptions
            .get_mut(subscription)
            .ok_or_else(|| BackendError::NotFound(subscription.to_string()))?;
        sub.spec.push_endpoint = push_endpoint.filter(|s| !s.is_empty());
        sub.spec.push_attributes = push_attributes;
        self.persist_locked(&state)?;
        Ok(())
    }

    async fn publish(
        &self,
        topic: &str,
        messages: Vec<NewMessage>,
    ) -> Result<Vec<String>, BackendError> {
        let mut state = self.state.write().await;
        let topic_state = state
            .topics
            .get_mut(topic)
            .ok_or_else(|| BackendError::NotFound(topic.to_string()))?;

        let now = Utc::now();
        let mut ids = Vec::with_capacity(messages.len());
        for message in messages {
            let id = Uuid::new_v4().to_string();
            topic_state.messages.push(RelayMessage {
                id: id.clone(),
                data: message.data,
                attributes: message.attributes,
                ordering_key: message.ordering_key,
                published_at: now,
            });
            ids.push(id);
        }
        self.persist_locked(&state)?;
        Ok(ids)
    }

    async fn pull(
        &self,
        subscription: &str,
        max_messages: u32,
    ) -> Result<Vec<Delivery>, BackendError> {
        let mut state = self.state.write().await;
        let mut sub = state
            .subscriptions
            .remove(subscription)
            .ok_or_else(|| BackendError::NotFound(subscription.to_string()))?;

        let result = (|| {
            let topic_name = sub.spec.topic.clone();
            if !state.topics.contains_key(&topic_name) {
                return Err(BackendError::FailedPrecondition(format!(
                    "topic {} no longer exists",
                    topic_name
                )));
            }

            let now = Utc::now();
            let expired: Vec<_> = sub
                .inflight
                .iter()
                .filter(|(_, entry)| entry.deadline <= now)
                .map(|(ack_id, entry)| (ack_id.clone(), entry.topic_index))
                .collect();
            for (ack_id, index) in expired {
                sub.inflight.remove(&ack_id);
                if !sub.acked.contains(&index) {
                    let attempt = *sub.attempts.get(&index).unwrap_or(&1);
                    let delay = Self::backoff_seconds(&sub.spec, attempt);
                    sub.retry_queue.push_back(RetryEntry {
                        topic_index: index,
                        available_at: now + ChronoDuration::seconds(delay as i64),
                    });
                }
            }

            let limit = max_messages.clamp(1, 1000) as usize;
            let mut deliveries = Vec::with_capacity(limit);
            let topic_len = state.topics.get(&topic_name).unwrap().messages.len();
            let mut skipped_ordering: HashSet<usize> = HashSet::new();

            while deliveries.len() < limit {
                let index = if let Some(pos) = sub.retry_queue.iter().position(|e| {
                    e.available_at <= now && !skipped_ordering.contains(&e.topic_index)
                }) {
                    sub.retry_queue.remove(pos).unwrap().topic_index
                } else if sub.next_index < topic_len {
                    let index = sub.next_index;
                    sub.next_index += 1;
                    index
                } else {
                    break;
                };

                if sub.acked.contains(&index)
                    || sub.inflight.values().any(|v| v.topic_index == index)
                {
                    continue;
                }

                let topic_state = state.topics.get(&topic_name).unwrap();
                if Self::ordering_blocked(&sub, topic_state, index, now) {
                    skipped_ordering.insert(index);
                    // Ready on the next pull once the predecessor is acked.
                    sub.retry_queue.push_back(RetryEntry {
                        topic_index: index,
                        available_at: now,
                    });
                    continue;
                }

                let attempt = sub.attempts.entry(index).or_insert(0);
                *attempt += 1;
                let current_attempt = *attempt;

                if let Some(dlq) = &sub.spec.dead_letter {
                    let max = dlq.max_delivery_attempts.clamp(5, 100);
                    if current_attempt > max {
                        let original =
                            state.topics.get(&topic_name).unwrap().messages[index].clone();
                        if let Some(dead_topic) = state.topics.get_mut(&dlq.topic) {
                            let mut attributes = original.attributes.clone();
                            attributes.insert(
                                "x-zyvor-dead-letter-source".into(),
                                subscription.to_string(),
                            );
                            attributes
                                .insert("x-zyvor-original-message-id".into(), original.id.clone());
                            dead_topic.messages.push(RelayMessage {
                                id: Uuid::new_v4().to_string(),
                                data: original.data,
                                attributes,
                                ordering_key: original.ordering_key,
                                published_at: Utc::now(),
                            });
                            sub.acked.insert(index);
                            continue;
                        }
                    }
                }

                let message = state.topics.get(&topic_name).unwrap().messages[index].clone();
                let ack_id = if sub.spec.enable_exactly_once_delivery {
                    format!("{subscription}:{index}:{current_attempt}")
                } else {
                    Uuid::new_v4().to_string()
                };
                let ack_deadline = Self::normalize_ack_deadline(sub.spec.ack_deadline_seconds);
                sub.inflight.insert(
                    ack_id.clone(),
                    Inflight {
                        topic_index: index,
                        deadline: now + ChronoDuration::seconds(ack_deadline as i64),
                    },
                );
                deliveries.push(Delivery {
                    ack_id,
                    message,
                    delivery_attempt: current_attempt,
                });
            }

            Ok(deliveries)
        })();

        state.subscriptions.insert(subscription.to_string(), sub);
        if result.is_ok() {
            self.persist_locked(&state)?;
        }
        result
    }

    async fn acknowledge(
        &self,
        subscription: &str,
        ack_ids: &[String],
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        let now = Utc::now();
        let topic_name = {
            let sub = state
                .subscriptions
                .get(subscription)
                .ok_or_else(|| BackendError::NotFound(subscription.to_string()))?;
            sub.spec.topic.clone()
        };
        for ack_id in ack_ids {
            let entry = {
                let sub = state
                    .subscriptions
                    .get_mut(subscription)
                    .ok_or_else(|| BackendError::NotFound(subscription.to_string()))?;
                match sub.inflight.remove(ack_id) {
                    Some(entry) => {
                        if sub.spec.enable_exactly_once_delivery && entry.deadline < now {
                            return Err(BackendError::FailedPrecondition(format!(
                                "ack deadline expired for {ack_id}"
                            )));
                        }
                        Some((entry, sub.spec.enable_exactly_once_delivery))
                    }
                    None if sub.spec.enable_exactly_once_delivery => None,
                    None => None,
                }
            };
            if let Some((entry, _)) = entry {
                let key = state
                    .topics
                    .get(&topic_name)
                    .and_then(|t| t.messages.get(entry.topic_index))
                    .map(|m| m.ordering_key.clone())
                    .unwrap_or_default();
                let sub = state.subscriptions.get_mut(subscription).unwrap();
                if !key.is_empty() {
                    sub.ordering_acked.insert(key, entry.topic_index);
                }
                sub.acked.insert(entry.topic_index);
            }
        }
        self.persist_locked(&state)?;
        Ok(())
    }

    async fn modify_ack_deadline(
        &self,
        subscription: &str,
        ack_ids: &[String],
        seconds: u32,
    ) -> Result<(), BackendError> {
        if seconds > 600 {
            return Err(BackendError::InvalidArgument(
                "ack_deadline_seconds must be <= 600".into(),
            ));
        }
        let mut state = self.state.write().await;
        let sub = state
            .subscriptions
            .get_mut(subscription)
            .ok_or_else(|| BackendError::NotFound(subscription.to_string()))?;
        let now = Utc::now();
        for ack_id in ack_ids {
            if seconds == 0 {
                if let Some(entry) = sub.inflight.remove(ack_id) {
                    let attempt = *sub.attempts.get(&entry.topic_index).unwrap_or(&1);
                    let delay = Self::backoff_seconds(&sub.spec, attempt);
                    sub.retry_queue.push_front(RetryEntry {
                        topic_index: entry.topic_index,
                        available_at: now + ChronoDuration::seconds(delay as i64),
                    });
                }
            } else if let Some(entry) = sub.inflight.get_mut(ack_id) {
                entry.deadline = now + ChronoDuration::seconds(seconds as i64);
            }
        }
        self.persist_locked(&state)?;
        Ok(())
    }

    async fn seek_to_time(
        &self,
        subscription: &str,
        time: DateTime<Utc>,
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        let topic_name = state
            .subscriptions
            .get(subscription)
            .ok_or_else(|| BackendError::NotFound(subscription.to_string()))?
            .spec
            .topic
            .clone();
        let messages = &state
            .topics
            .get(&topic_name)
            .ok_or_else(|| BackendError::NotFound(topic_name.clone()))?
            .messages;
        let index = messages
            .iter()
            .position(|message| message.published_at >= time)
            .unwrap_or(messages.len());
        let sub = state.subscriptions.get_mut(subscription).unwrap();
        sub.next_index = index;
        sub.acked.clear();
        sub.inflight.clear();
        sub.retry_queue.clear();
        sub.attempts.clear();
        sub.ordering_acked.clear();
        self.persist_locked(&state)?;
        Ok(())
    }

    async fn seek_to_snapshot(
        &self,
        subscription: &str,
        snapshot: &str,
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        let snap = state
            .snapshots
            .get(snapshot)
            .ok_or_else(|| BackendError::NotFound(snapshot.to_string()))?
            .clone();
        let sub = state
            .subscriptions
            .get_mut(subscription)
            .ok_or_else(|| BackendError::NotFound(subscription.to_string()))?;
        if sub.spec.topic != snap.topic {
            return Err(BackendError::FailedPrecondition(
                "snapshot topic does not match subscription topic".into(),
            ));
        }
        sub.next_index = snap.topic_index;
        sub.acked.clear();
        sub.inflight.clear();
        sub.retry_queue.clear();
        sub.attempts.clear();
        sub.ordering_acked.clear();
        self.persist_locked(&state)?;
        Ok(())
    }

    async fn create_snapshot(
        &self,
        name: &str,
        subscription: &str,
        labels: HashMap<String, String>,
    ) -> Result<SnapshotSpec, BackendError> {
        if !name.starts_with("projects/") || !name.contains("/snapshots/") {
            return Err(BackendError::InvalidArgument(format!(
                "invalid snapshot name {name}"
            )));
        }
        let mut state = self.state.write().await;
        if state.snapshots.contains_key(name) {
            return Err(BackendError::AlreadyExists(name.to_string()));
        }
        let sub = state
            .subscriptions
            .get(subscription)
            .ok_or_else(|| BackendError::NotFound(subscription.to_string()))?;
        let spec = SnapshotSpec {
            name: name.to_string(),
            topic: sub.spec.topic.clone(),
            expire_time: Utc::now() + ChronoDuration::days(7),
            labels,
            topic_index: sub.next_index,
        };
        state.snapshots.insert(name.to_string(), spec.clone());
        self.persist_locked(&state)?;
        Ok(spec)
    }

    async fn update_snapshot(
        &self,
        snapshot: SnapshotSpec,
        update_mask: &[String],
    ) -> Result<SnapshotSpec, BackendError> {
        let mut state = self.state.write().await;
        let entry = state
            .snapshots
            .get_mut(&snapshot.name)
            .ok_or_else(|| BackendError::NotFound(snapshot.name.clone()))?;
        if update_mask.is_empty() || update_mask.iter().any(|f| f == "*" || f == "labels") {
            entry.labels = snapshot.labels;
        }
        let out = entry.clone();
        self.persist_locked(&state)?;
        Ok(out)
    }

    async fn get_snapshot(&self, name: &str) -> Result<SnapshotSpec, BackendError> {
        let state = self.state.read().await;
        state
            .snapshots
            .get(name)
            .cloned()
            .ok_or_else(|| BackendError::NotFound(name.to_string()))
    }

    async fn list_snapshots(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SnapshotSpec>, BackendError> {
        let state = self.state.read().await;
        let mut items: Vec<_> = state
            .snapshots
            .values()
            .filter(|s| Self::belongs_to_project(&s.name, project))
            .cloned()
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(paginate(items, page_size, page_token))
    }

    async fn delete_snapshot(&self, name: &str) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        if state.snapshots.remove(name).is_none() {
            return Err(BackendError::NotFound(name.to_string()));
        }
        self.persist_locked(&state)?;
        Ok(())
    }

    async fn get_iam_policy(&self, resource: &str) -> Result<IamPolicy, BackendError> {
        let state = self.state.read().await;
        Ok(state
            .iam
            .get(resource)
            .cloned()
            .unwrap_or_else(|| IamPolicy {
                version: 1,
                bindings: vec![IamBinding {
                    role: "roles/pubsub.admin".into(),
                    members: vec!["allAuthenticatedUsers".into()],
                }],
                etag: "default".into(),
            }))
    }

    async fn set_iam_policy(
        &self,
        resource: &str,
        mut policy: IamPolicy,
    ) -> Result<IamPolicy, BackendError> {
        let mut state = self.state.write().await;
        if policy.etag.is_empty() {
            policy.etag = Uuid::new_v4().to_string();
        }
        if policy.version == 0 {
            policy.version = 1;
        }
        state.iam.insert(resource.to_string(), policy.clone());
        self.persist_locked(&state)?;
        Ok(policy)
    }

    async fn test_iam_permissions(
        &self,
        resource: &str,
        permissions: &[String],
    ) -> Result<Vec<String>, BackendError> {
        let _ = self.get_iam_policy(resource).await?;
        // Compatibility subset: grant all requested permissions when a policy exists.
        Ok(permissions.to_vec())
    }

    async fn create_schema(&self, schema: SchemaSpec) -> Result<SchemaSpec, BackendError> {
        if !schema.name.starts_with("projects/") || !schema.name.contains("/schemas/") {
            return Err(BackendError::InvalidArgument(format!(
                "invalid schema name {}",
                schema.name
            )));
        }
        self.validate_schema(&schema).await?;
        let mut state = self.state.write().await;
        if state.schemas.contains_key(&schema.name) {
            return Err(BackendError::AlreadyExists(schema.name));
        }
        state.schemas.insert(schema.name.clone(), schema.clone());
        self.persist_locked(&state)?;
        Ok(schema)
    }

    async fn get_schema(&self, name: &str) -> Result<SchemaSpec, BackendError> {
        let state = self.state.read().await;
        state
            .schemas
            .get(name)
            .cloned()
            .ok_or_else(|| BackendError::NotFound(name.to_string()))
    }

    async fn list_schemas(
        &self,
        parent: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SchemaSpec>, BackendError> {
        let state = self.state.read().await;
        let mut items: Vec<_> = state
            .schemas
            .values()
            .filter(|s| Self::belongs_to_project(&s.name, parent))
            .cloned()
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(paginate(items, page_size, page_token))
    }

    async fn delete_schema(&self, name: &str) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        if state.schemas.remove(name).is_none() {
            return Err(BackendError::NotFound(name.to_string()));
        }
        self.persist_locked(&state)?;
        Ok(())
    }

    async fn validate_schema(&self, schema: &SchemaSpec) -> Result<(), BackendError> {
        if schema.definition.trim().is_empty() {
            return Err(BackendError::InvalidArgument(
                "schema definition is required".into(),
            ));
        }
        match schema.schema_type.as_str() {
            "PROTOCOL_BUFFER" | "AVRO" | "UNSPECIFIED" | "" => Ok(()),
            other => Err(BackendError::InvalidArgument(format!(
                "unsupported schema type {other}"
            ))),
        }
    }

    async fn validate_message(
        &self,
        schema_name: Option<&str>,
        schema: Option<&SchemaSpec>,
        message: &[u8],
    ) -> Result<(), BackendError> {
        let resolved = if let Some(s) = schema {
            s.clone()
        } else if let Some(name) = schema_name {
            self.get_schema(name).await?
        } else {
            return Err(BackendError::InvalidArgument(
                "schema name or schema body is required".into(),
            ));
        };
        self.validate_schema(&resolved).await?;
        if message.is_empty() {
            return Err(BackendError::InvalidArgument(
                "message body is required".into(),
            ));
        }
        // Compatibility subset: accept any non-empty payload when schema exists.
        Ok(())
    }

    async fn list_push_subscriptions(&self) -> Result<Vec<SubscriptionSpec>, BackendError> {
        let state = self.state.read().await;
        Ok(state
            .subscriptions
            .values()
            .filter(|s| s.spec.push_endpoint.is_some())
            .map(|s| s.spec.clone())
            .collect())
    }

    async fn inventory(&self, project: &str) -> Result<InventoryReport, BackendError> {
        let state = self.state.read().await;
        let mut topics: Vec<TopicInventory> = state
            .topics
            .iter()
            .filter(|(name, _)| Self::belongs_to_project(name, project))
            .map(|(name, topic)| {
                let recent: Vec<MessagePreview> = topic
                    .messages
                    .iter()
                    .rev()
                    .take(20)
                    .map(|m| MessagePreview {
                        id: m.id.clone(),
                        data: String::from_utf8_lossy(&m.data).into_owned(),
                        attributes: m.attributes.clone(),
                        ordering_key: m.ordering_key.clone(),
                        published_at: m.published_at,
                    })
                    .collect();
                TopicInventory {
                    name: name.clone(),
                    labels: topic.spec.labels.clone(),
                    message_count: topic.messages.len() as u64,
                    recent,
                }
            })
            .collect();
        topics.sort_by(|a, b| a.name.cmp(&b.name));

        let mut subscriptions: Vec<SubscriptionInventory> = state
            .subscriptions
            .iter()
            .filter(|(name, _)| Self::belongs_to_project(name, project))
            .map(|(name, sub)| {
                let topic_len = state
                    .topics
                    .get(&sub.spec.topic)
                    .map(|t| t.messages.len())
                    .unwrap_or(0);
                let unacked_behind_cursor = (0..sub.next_index.min(topic_len))
                    .filter(|i| !sub.acked.contains(i))
                    .count();
                let ahead = topic_len.saturating_sub(sub.next_index);
                let backlog = (unacked_behind_cursor + ahead + sub.retry_queue.len()) as u64;
                SubscriptionInventory {
                    name: name.clone(),
                    topic: sub.spec.topic.clone(),
                    ack_deadline_seconds: sub.spec.ack_deadline_seconds,
                    enable_message_ordering: sub.spec.enable_message_ordering,
                    enable_exactly_once_delivery: sub.spec.enable_exactly_once_delivery,
                    push_endpoint: sub.spec.push_endpoint.clone(),
                    push_attributes: sub.spec.push_attributes.clone(),
                    dead_letter_topic: sub.spec.dead_letter.as_ref().map(|d| d.topic.clone()),
                    topic_message_count: topic_len as u64,
                    next_index: sub.next_index as u64,
                    backlog,
                    inflight: sub.inflight.len() as u64,
                    retry_queued: sub.retry_queue.len() as u64,
                    acked: sub.acked.len() as u64,
                }
            })
            .collect();
        subscriptions.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(InventoryReport {
            topics,
            subscriptions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeadLetterSpec, NewMessage, RetrySpec, SubscriptionSpec, TopicSpec};
    use std::collections::HashMap;

    fn topic(name: &str) -> TopicSpec {
        TopicSpec {
            name: name.into(),
            labels: HashMap::new(),
            kms_key_name: String::new(),
        }
    }

    fn subscription(name: &str, topic: &str) -> SubscriptionSpec {
        SubscriptionSpec {
            name: name.into(),
            topic: topic.into(),
            ack_deadline_seconds: 10,
            labels: HashMap::new(),
            enable_message_ordering: false,
            enable_exactly_once_delivery: false,
            dead_letter: None,
            retry: None,
            push_endpoint: None,
            push_attributes: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn publish_pull_ack() {
        let backend = MemoryBackend::new();
        let topic_name = "projects/demo/topics/orders";
        let sub_name = "projects/demo/subscriptions/orders-worker";
        backend.create_topic(topic(topic_name)).await.unwrap();
        backend
            .create_subscription(subscription(sub_name, topic_name))
            .await
            .unwrap();
        backend
            .publish(
                topic_name,
                vec![NewMessage {
                    data: b"hello".to_vec(),
                    attributes: HashMap::new(),
                    ordering_key: String::new(),
                }],
            )
            .await
            .unwrap();
        let deliveries = backend.pull(sub_name, 10).await.unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].message.data, b"hello");
        backend
            .acknowledge(sub_name, &[deliveries[0].ack_id.clone()])
            .await
            .unwrap();
        assert!(backend.pull(sub_name, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn nack_redelivers() {
        let backend = MemoryBackend::new();
        let topic_name = "projects/demo/topics/jobs";
        let sub_name = "projects/demo/subscriptions/jobs-worker";
        backend.create_topic(topic(topic_name)).await.unwrap();
        let mut sub = subscription(sub_name, topic_name);
        sub.retry = Some(RetrySpec {
            minimum_backoff_seconds: 0,
            maximum_backoff_seconds: 0,
        });
        backend.create_subscription(sub).await.unwrap();
        backend
            .publish(
                topic_name,
                vec![NewMessage {
                    data: b"job".to_vec(),
                    attributes: HashMap::new(),
                    ordering_key: String::new(),
                }],
            )
            .await
            .unwrap();
        let first = backend.pull(sub_name, 1).await.unwrap().remove(0);
        backend
            .modify_ack_deadline(sub_name, &[first.ack_id], 0)
            .await
            .unwrap();
        let second = backend.pull(sub_name, 1).await.unwrap().remove(0);
        assert_eq!(second.delivery_attempt, 2);
    }

    #[tokio::test]
    async fn ordering_key_blocks_until_ack() {
        let backend = MemoryBackend::new();
        let topic_name = "projects/demo/topics/ordered";
        let sub_name = "projects/demo/subscriptions/ordered-worker";
        backend.create_topic(topic(topic_name)).await.unwrap();
        let mut sub = subscription(sub_name, topic_name);
        sub.enable_message_ordering = true;
        backend.create_subscription(sub).await.unwrap();
        backend
            .publish(
                topic_name,
                vec![
                    NewMessage {
                        data: b"1".to_vec(),
                        attributes: HashMap::new(),
                        ordering_key: "k".into(),
                    },
                    NewMessage {
                        data: b"2".to_vec(),
                        attributes: HashMap::new(),
                        ordering_key: "k".into(),
                    },
                ],
            )
            .await
            .unwrap();
        let first = backend.pull(sub_name, 10).await.unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].message.data, b"1");
        backend
            .acknowledge(sub_name, &[first[0].ack_id.clone()])
            .await
            .unwrap();
        let second = backend.pull(sub_name, 10).await.unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].message.data, b"2");
    }

    #[tokio::test]
    async fn snapshot_seek_restores_cursor() {
        let backend = MemoryBackend::new();
        let topic_name = "projects/demo/topics/snap-topic";
        let sub_name = "projects/demo/subscriptions/snap-sub";
        let snap_name = "projects/demo/snapshots/snap1";
        backend.create_topic(topic(topic_name)).await.unwrap();
        backend
            .create_subscription(subscription(sub_name, topic_name))
            .await
            .unwrap();
        backend
            .publish(
                topic_name,
                vec![NewMessage {
                    data: b"a".to_vec(),
                    attributes: HashMap::new(),
                    ordering_key: String::new(),
                }],
            )
            .await
            .unwrap();
        let d = backend.pull(sub_name, 1).await.unwrap();
        backend
            .acknowledge(sub_name, &[d[0].ack_id.clone()])
            .await
            .unwrap();
        backend
            .create_snapshot(snap_name, sub_name, HashMap::new())
            .await
            .unwrap();
        backend
            .publish(
                topic_name,
                vec![NewMessage {
                    data: b"b".to_vec(),
                    attributes: HashMap::new(),
                    ordering_key: String::new(),
                }],
            )
            .await
            .unwrap();
        let _ = backend.pull(sub_name, 1).await.unwrap();
        backend.seek_to_snapshot(sub_name, snap_name).await.unwrap();
        let again = backend.pull(sub_name, 10).await.unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].message.data, b"b");
    }

    #[tokio::test]
    async fn persistence_round_trip() {
        let dir = std::env::temp_dir().join(format!("relay-pubsub-{}", Uuid::new_v4()));
        let path = dir.join("state.json");
        let topic_name = "projects/demo/topics/persist";
        {
            let backend = MemoryBackend::with_persistence(&path).unwrap();
            backend.create_topic(topic(topic_name)).await.unwrap();
        }
        let backend = MemoryBackend::with_persistence(&path).unwrap();
        assert_eq!(
            backend.get_topic(topic_name).await.unwrap().name,
            topic_name
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn dead_letter_after_max_attempts() {
        let backend = MemoryBackend::new();
        let source = "projects/demo/topics/source";
        let dlq = "projects/demo/topics/dlq";
        let sub_name = "projects/demo/subscriptions/source-worker";
        backend.create_topic(topic(source)).await.unwrap();
        backend.create_topic(topic(dlq)).await.unwrap();
        let mut sub = subscription(sub_name, source);
        sub.dead_letter = Some(DeadLetterSpec {
            topic: dlq.into(),
            max_delivery_attempts: 5,
        });
        sub.retry = Some(RetrySpec {
            minimum_backoff_seconds: 0,
            maximum_backoff_seconds: 0,
        });
        backend.create_subscription(sub).await.unwrap();
        backend
            .publish(
                source,
                vec![NewMessage {
                    data: b"poison".to_vec(),
                    attributes: HashMap::new(),
                    ordering_key: String::new(),
                }],
            )
            .await
            .unwrap();
        for _ in 0..5 {
            let d = backend.pull(sub_name, 1).await.unwrap().remove(0);
            backend
                .modify_ack_deadline(sub_name, &[d.ack_id], 0)
                .await
                .unwrap();
        }
        assert!(backend.pull(sub_name, 1).await.unwrap().is_empty());
        let dlq_sub = "projects/demo/subscriptions/dlq-reader";
        backend
            .create_subscription(subscription(dlq_sub, dlq))
            .await
            .unwrap();
        backend
            .publish(
                source,
                vec![NewMessage {
                    data: b"poison2".to_vec(),
                    attributes: HashMap::new(),
                    ordering_key: String::new(),
                }],
            )
            .await
            .unwrap();
        for _ in 0..5 {
            let d = backend.pull(sub_name, 1).await.unwrap().remove(0);
            backend
                .modify_ack_deadline(sub_name, &[d.ack_id], 0)
                .await
                .unwrap();
        }
        let _ = backend.pull(sub_name, 1).await.unwrap();
        assert_eq!(backend.pull(dlq_sub, 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn pagination_works() {
        let backend = MemoryBackend::new();
        for i in 0..5 {
            backend
                .create_topic(topic(&format!("projects/demo/topics/t{i}")))
                .await
                .unwrap();
        }
        let page1 = backend.list_topics("projects/demo", 2, "").await.unwrap();
        assert_eq!(page1.items.len(), 2);
        assert!(!page1.next_page_token.is_empty());
        let page2 = backend
            .list_topics("projects/demo", 2, &page1.next_page_token)
            .await
            .unwrap();
        assert_eq!(page2.items.len(), 2);
    }
}
