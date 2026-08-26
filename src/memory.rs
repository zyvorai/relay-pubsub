use crate::backend::{BackendError, RelayBackend};
use crate::model::{Delivery, NewMessage, RelayMessage, SubscriptionSpec, TopicSpec};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
pub struct MemoryBackend {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    topics: HashMap<String, TopicState>,
    subscriptions: HashMap<String, SubscriptionState>,
}

struct TopicState {
    spec: TopicSpec,
    messages: Vec<RelayMessage>,
}

struct SubscriptionState {
    spec: SubscriptionSpec,
    next_index: usize,
    acked: HashSet<usize>,
    inflight: HashMap<String, Inflight>,
    retry_queue: VecDeque<usize>,
    attempts: HashMap<usize, u32>,
}

struct Inflight {
    topic_index: usize,
    deadline: DateTime<Utc>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    fn belongs_to_project(resource: &str, project: &str) -> bool {
        resource.starts_with(&format!("{project}/"))
    }

    fn normalize_ack_deadline(seconds: u32) -> u32 {
        seconds.clamp(10, 600)
    }
}

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
        Ok(topic)
    }

    async fn get_topic(&self, name: &str) -> Result<TopicSpec, BackendError> {
        let state = self.state.read().await;
        state
            .topics
            .get(name)
            .map(|t| t.spec.clone())
            .ok_or_else(|| BackendError::NotFound(name.to_string()))
    }

    async fn list_topics(&self, project: &str) -> Result<Vec<TopicSpec>, BackendError> {
        let state = self.state.read().await;
        let mut topics: Vec<_> = state
            .topics
            .values()
            .filter(|t| Self::belongs_to_project(&t.spec.name, project))
            .map(|t| t.spec.clone())
            .collect();
        topics.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(topics)
    }

    async fn delete_topic(&self, name: &str) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        if state.topics.remove(name).is_none() {
            return Err(BackendError::NotFound(name.to_string()));
        }
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
            },
        );
        Ok(spec)
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
    ) -> Result<Vec<SubscriptionSpec>, BackendError> {
        let state = self.state.read().await;
        let mut subscriptions: Vec<_> = state
            .subscriptions
            .values()
            .filter(|s| Self::belongs_to_project(&s.spec.name, project))
            .map(|s| s.spec.clone())
            .collect();
        subscriptions.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(subscriptions)
    }

    async fn delete_subscription(&self, name: &str) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        if state.subscriptions.remove(name).is_none() {
            return Err(BackendError::NotFound(name.to_string()));
        }
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
                    sub.retry_queue.push_back(index);
                }
            }

            let limit = max_messages.clamp(1, 1000) as usize;
            let mut deliveries = Vec::with_capacity(limit);
            let topic_len = state.topics.get(&topic_name).unwrap().messages.len();

            while deliveries.len() < limit {
                let index = if let Some(index) = sub.retry_queue.pop_front() {
                    index
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
                let ack_id = Uuid::new_v4().to_string();
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
        result
    }

    async fn acknowledge(
        &self,
        subscription: &str,
        ack_ids: &[String],
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().await;
        let sub = state
            .subscriptions
            .get_mut(subscription)
            .ok_or_else(|| BackendError::NotFound(subscription.to_string()))?;
        for ack_id in ack_ids {
            if let Some(entry) = sub.inflight.remove(ack_id) {
                sub.acked.insert(entry.topic_index);
            }
        }
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
                    sub.retry_queue.push_front(entry.topic_index);
                }
            } else if let Some(entry) = sub.inflight.get_mut(ack_id) {
                entry.deadline = now + ChronoDuration::seconds(seconds as i64);
            }
        }
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeadLetterSpec, NewMessage, SubscriptionSpec, TopicSpec};
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
        backend
            .create_subscription(subscription(sub_name, topic_name))
            .await
            .unwrap();
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
        // Subscriptions receive messages published after creation, so publish another forced DLQ sequence.
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
}
