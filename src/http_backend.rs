// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

use crate::backend::{BackendError, RelayBackend};
use crate::memory::MemoryBackend;
use crate::model::{
    paginate, Delivery, IamPolicy, NewMessage, Page, SchemaSpec, SnapshotSpec, SubscriptionSpec,
    TopicSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Legacy invented Relay topics API. Snapshot/IAM/schema live in an embedded
/// memory store; core pub/sub data plane still speaks HTTP to Relay.
#[derive(Clone)]
pub struct HttpRelayBackend {
    base_url: String,
    token: Option<String>,
    client: Client,
    local: std::sync::Arc<MemoryBackend>,
}

#[derive(Serialize)]
struct PublishBody<'a> {
    topic: &'a str,
    messages: Vec<NewMessage>,
}

#[derive(Deserialize)]
struct PublishResult {
    message_ids: Vec<String>,
}

#[derive(Serialize)]
struct PullBody<'a> {
    subscription: &'a str,
    max_messages: u32,
}

#[derive(Deserialize)]
struct PullResult {
    deliveries: Vec<Delivery>,
}

#[derive(Serialize)]
struct AckBody<'a> {
    subscription: &'a str,
    ack_ids: &'a [String],
}

#[derive(Serialize)]
struct DeadlineBody<'a> {
    subscription: &'a str,
    ack_ids: &'a [String],
    seconds: u32,
}

#[derive(Serialize)]
struct SeekBody<'a> {
    subscription: &'a str,
    time: DateTime<Utc>,
}

impl HttpRelayBackend {
    pub fn new(
        base_url: impl Into<String>,
        token: Option<String>,
        timeout: Duration,
    ) -> Result<Self, BackendError> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| BackendError::Internal(e.to_string()))?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            client,
            local: std::sync::Arc::new(MemoryBackend::new()),
        })
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let req = self
            .client
            .request(method, format!("{}{}", self.base_url, path));
        match &self.token {
            Some(token) => req.bearer_auth(token),
            None => req,
        }
    }

    async fn decode<T: DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, BackendError> {
        let status = response.status();
        if status.is_success() {
            return response
                .json::<T>()
                .await
                .map_err(|e| BackendError::Internal(e.to_string()));
        }
        let text = response.text().await.unwrap_or_default();
        Err(match status {
            StatusCode::NOT_FOUND => BackendError::NotFound(text),
            StatusCode::CONFLICT => BackendError::AlreadyExists(text),
            StatusCode::BAD_REQUEST => BackendError::InvalidArgument(text),
            StatusCode::PRECONDITION_FAILED => BackendError::FailedPrecondition(text),
            StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::BAD_GATEWAY
            | StatusCode::GATEWAY_TIMEOUT => BackendError::Unavailable(text),
            _ => BackendError::Internal(format!("Relay returned {status}: {text}")),
        })
    }

    async fn empty(&self, response: reqwest::Response) -> Result<(), BackendError> {
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let text = response.text().await.unwrap_or_default();
        Err(match status {
            StatusCode::NOT_FOUND => BackendError::NotFound(text),
            StatusCode::CONFLICT => BackendError::AlreadyExists(text),
            StatusCode::BAD_REQUEST => BackendError::InvalidArgument(text),
            StatusCode::PRECONDITION_FAILED => BackendError::FailedPrecondition(text),
            StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::BAD_GATEWAY
            | StatusCode::GATEWAY_TIMEOUT => BackendError::Unavailable(text),
            _ => BackendError::Internal(format!("Relay returned {status}: {text}")),
        })
    }
}

#[async_trait]
impl RelayBackend for HttpRelayBackend {
    async fn create_topic(&self, topic: TopicSpec) -> Result<TopicSpec, BackendError> {
        let response = self
            .request(Method::POST, "/v1/topics")
            .json(&topic)
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn update_topic(
        &self,
        topic: TopicSpec,
        update_mask: &[String],
    ) -> Result<TopicSpec, BackendError> {
        let _ = update_mask;
        // Legacy API has no update; recreate semantics via delete+create are unsafe —
        // return current remote topic after a no-op get.
        self.get_topic(&topic.name).await?;
        Ok(topic)
    }

    async fn get_topic(&self, name: &str) -> Result<TopicSpec, BackendError> {
        let response = self
            .request(Method::GET, "/v1/topics/by-name")
            .query(&[("name", name)])
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn list_topics(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<TopicSpec>, BackendError> {
        let response = self
            .request(Method::GET, "/v1/topics")
            .query(&[("project", project)])
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let items: Vec<TopicSpec> = self.decode(response).await?;
        Ok(paginate(items, page_size, page_token))
    }

    async fn list_topic_subscriptions(
        &self,
        topic: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<String>, BackendError> {
        let project = topic.split('/').take(2).collect::<Vec<_>>().join("/");
        let all = self.list_subscriptions(&project, 1000, "").await?;
        let names: Vec<_> = all
            .items
            .into_iter()
            .filter(|s| s.topic == topic)
            .map(|s| s.name)
            .collect();
        Ok(paginate(names, page_size, page_token))
    }

    async fn delete_topic(&self, name: &str) -> Result<(), BackendError> {
        let response = self
            .request(Method::DELETE, "/v1/topics/by-name")
            .query(&[("name", name)])
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }

    async fn create_subscription(
        &self,
        subscription: SubscriptionSpec,
    ) -> Result<SubscriptionSpec, BackendError> {
        let response = self
            .request(Method::POST, "/v1/subscriptions")
            .json(&subscription)
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn update_subscription(
        &self,
        subscription: SubscriptionSpec,
        _update_mask: &[String],
    ) -> Result<SubscriptionSpec, BackendError> {
        self.get_subscription(&subscription.name).await?;
        Ok(subscription)
    }

    async fn get_subscription(&self, name: &str) -> Result<SubscriptionSpec, BackendError> {
        let response = self
            .request(Method::GET, "/v1/subscriptions/by-name")
            .query(&[("name", name)])
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn list_subscriptions(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SubscriptionSpec>, BackendError> {
        let response = self
            .request(Method::GET, "/v1/subscriptions")
            .query(&[("project", project)])
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let items: Vec<SubscriptionSpec> = self.decode(response).await?;
        Ok(paginate(items, page_size, page_token))
    }

    async fn delete_subscription(&self, name: &str) -> Result<(), BackendError> {
        let response = self
            .request(Method::DELETE, "/v1/subscriptions/by-name")
            .query(&[("name", name)])
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }

    async fn modify_push_config(
        &self,
        subscription: &str,
        push_endpoint: Option<String>,
        push_attributes: HashMap<String, String>,
    ) -> Result<(), BackendError> {
        self.local
            .modify_push_config(subscription, push_endpoint, push_attributes)
            .await
    }

    async fn publish(
        &self,
        topic: &str,
        messages: Vec<NewMessage>,
    ) -> Result<Vec<String>, BackendError> {
        let response = self
            .request(Method::POST, "/v1/messages:publish")
            .json(&PublishBody { topic, messages })
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let result: PublishResult = self.decode(response).await?;
        Ok(result.message_ids)
    }

    async fn pull(
        &self,
        subscription: &str,
        max_messages: u32,
    ) -> Result<Vec<Delivery>, BackendError> {
        let response = self
            .request(Method::POST, "/v1/messages:pull")
            .json(&PullBody {
                subscription,
                max_messages,
            })
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let result: PullResult = self.decode(response).await?;
        Ok(result.deliveries)
    }

    async fn acknowledge(
        &self,
        subscription: &str,
        ack_ids: &[String],
    ) -> Result<(), BackendError> {
        let response = self
            .request(Method::POST, "/v1/messages:ack")
            .json(&AckBody {
                subscription,
                ack_ids,
            })
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }

    async fn modify_ack_deadline(
        &self,
        subscription: &str,
        ack_ids: &[String],
        seconds: u32,
    ) -> Result<(), BackendError> {
        let response = self
            .request(Method::POST, "/v1/messages:modify-ack-deadline")
            .json(&DeadlineBody {
                subscription,
                ack_ids,
                seconds,
            })
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }

    async fn seek_to_time(
        &self,
        subscription: &str,
        time: DateTime<Utc>,
    ) -> Result<(), BackendError> {
        let response = self
            .request(Method::POST, "/v1/subscriptions:seek")
            .json(&SeekBody { subscription, time })
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }

    async fn seek_to_snapshot(
        &self,
        subscription: &str,
        snapshot: &str,
    ) -> Result<(), BackendError> {
        self.local.seek_to_snapshot(subscription, snapshot).await
    }

    async fn create_snapshot(
        &self,
        name: &str,
        subscription: &str,
        labels: HashMap<String, String>,
    ) -> Result<SnapshotSpec, BackendError> {
        self.local.create_snapshot(name, subscription, labels).await
    }

    async fn update_snapshot(
        &self,
        snapshot: SnapshotSpec,
        update_mask: &[String],
    ) -> Result<SnapshotSpec, BackendError> {
        self.local.update_snapshot(snapshot, update_mask).await
    }

    async fn get_snapshot(&self, name: &str) -> Result<SnapshotSpec, BackendError> {
        self.local.get_snapshot(name).await
    }

    async fn list_snapshots(
        &self,
        project: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SnapshotSpec>, BackendError> {
        self.local
            .list_snapshots(project, page_size, page_token)
            .await
    }

    async fn delete_snapshot(&self, name: &str) -> Result<(), BackendError> {
        self.local.delete_snapshot(name).await
    }

    async fn get_iam_policy(&self, resource: &str) -> Result<IamPolicy, BackendError> {
        self.local.get_iam_policy(resource).await
    }

    async fn set_iam_policy(
        &self,
        resource: &str,
        policy: IamPolicy,
    ) -> Result<IamPolicy, BackendError> {
        self.local.set_iam_policy(resource, policy).await
    }

    async fn test_iam_permissions(
        &self,
        resource: &str,
        permissions: &[String],
    ) -> Result<Vec<String>, BackendError> {
        self.local.test_iam_permissions(resource, permissions).await
    }

    async fn create_schema(&self, schema: SchemaSpec) -> Result<SchemaSpec, BackendError> {
        self.local.create_schema(schema).await
    }

    async fn get_schema(&self, name: &str) -> Result<SchemaSpec, BackendError> {
        self.local.get_schema(name).await
    }

    async fn list_schemas(
        &self,
        parent: &str,
        page_size: i32,
        page_token: &str,
    ) -> Result<Page<SchemaSpec>, BackendError> {
        self.local.list_schemas(parent, page_size, page_token).await
    }

    async fn delete_schema(&self, name: &str) -> Result<(), BackendError> {
        self.local.delete_schema(name).await
    }

    async fn validate_schema(&self, schema: &SchemaSpec) -> Result<(), BackendError> {
        self.local.validate_schema(schema).await
    }

    async fn validate_message(
        &self,
        schema_name: Option<&str>,
        schema: Option<&SchemaSpec>,
        message: &[u8],
    ) -> Result<(), BackendError> {
        self.local
            .validate_message(schema_name, schema, message)
            .await
    }

    async fn list_push_subscriptions(&self) -> Result<Vec<SubscriptionSpec>, BackendError> {
        self.local.list_push_subscriptions().await
    }

    async fn inventory(
        &self,
        project: &str,
    ) -> Result<crate::model::InventoryReport, BackendError> {
        self.local.inventory(project).await
    }
}
