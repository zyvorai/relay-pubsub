use crate::backend::{BackendError, RelayBackend};
use crate::model::{Delivery, NewMessage, SubscriptionSpec, TopicSpec};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct HttpRelayBackend {
    base_url: String,
    token: Option<String>,
    client: Client,
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
    pub fn new(base_url: impl Into<String>, token: Option<String>, timeout: Duration) -> Result<Self, BackendError> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| BackendError::Internal(e.to_string()))?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            client,
        })
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let req = self.client.request(method, format!("{}{}", self.base_url, path));
        match &self.token {
            Some(token) => req.bearer_auth(token),
            None => req,
        }
    }

    async fn decode<T: DeserializeOwned>(&self, response: reqwest::Response) -> Result<T, BackendError> {
        let status = response.status();
        if status.is_success() {
            return response.json::<T>().await.map_err(|e| BackendError::Internal(e.to_string()));
        }
        let text = response.text().await.unwrap_or_default();
        Err(match status {
            StatusCode::NOT_FOUND => BackendError::NotFound(text),
            StatusCode::CONFLICT => BackendError::AlreadyExists(text),
            StatusCode::BAD_REQUEST => BackendError::InvalidArgument(text),
            StatusCode::PRECONDITION_FAILED => BackendError::FailedPrecondition(text),
            StatusCode::SERVICE_UNAVAILABLE | StatusCode::BAD_GATEWAY | StatusCode::GATEWAY_TIMEOUT => {
                BackendError::Unavailable(text)
            }
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
            StatusCode::SERVICE_UNAVAILABLE | StatusCode::BAD_GATEWAY | StatusCode::GATEWAY_TIMEOUT => BackendError::Unavailable(text),
            _ => BackendError::Internal(format!("Relay returned {status}: {text}")),
        })
    }
}

#[async_trait]
impl RelayBackend for HttpRelayBackend {
    async fn create_topic(&self, topic: TopicSpec) -> Result<TopicSpec, BackendError> {
        let response = self.request(Method::POST, "/v1/topics").json(&topic).send().await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn get_topic(&self, name: &str) -> Result<TopicSpec, BackendError> {
        let response = self.request(Method::GET, "/v1/topics/by-name").query(&[("name", name)]).send().await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn list_topics(&self, project: &str) -> Result<Vec<TopicSpec>, BackendError> {
        let response = self.request(Method::GET, "/v1/topics").query(&[("project", project)]).send().await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn delete_topic(&self, name: &str) -> Result<(), BackendError> {
        let response = self.request(Method::DELETE, "/v1/topics/by-name").query(&[("name", name)]).send().await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }

    async fn create_subscription(&self, subscription: SubscriptionSpec) -> Result<SubscriptionSpec, BackendError> {
        let response = self.request(Method::POST, "/v1/subscriptions").json(&subscription).send().await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn get_subscription(&self, name: &str) -> Result<SubscriptionSpec, BackendError> {
        let response = self.request(Method::GET, "/v1/subscriptions/by-name").query(&[("name", name)]).send().await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn list_subscriptions(&self, project: &str) -> Result<Vec<SubscriptionSpec>, BackendError> {
        let response = self.request(Method::GET, "/v1/subscriptions").query(&[("project", project)]).send().await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.decode(response).await
    }

    async fn delete_subscription(&self, name: &str) -> Result<(), BackendError> {
        let response = self.request(Method::DELETE, "/v1/subscriptions/by-name").query(&[("name", name)]).send().await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }

    async fn publish(&self, topic: &str, messages: Vec<NewMessage>) -> Result<Vec<String>, BackendError> {
        let response = self.request(Method::POST, "/v1/messages:publish")
            .json(&PublishBody { topic, messages })
            .send().await.map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let result: PublishResult = self.decode(response).await?;
        Ok(result.message_ids)
    }

    async fn pull(&self, subscription: &str, max_messages: u32) -> Result<Vec<Delivery>, BackendError> {
        let response = self.request(Method::POST, "/v1/messages:pull")
            .json(&PullBody { subscription, max_messages })
            .send().await.map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let result: PullResult = self.decode(response).await?;
        Ok(result.deliveries)
    }

    async fn acknowledge(&self, subscription: &str, ack_ids: &[String]) -> Result<(), BackendError> {
        let response = self.request(Method::POST, "/v1/messages:ack")
            .json(&AckBody { subscription, ack_ids })
            .send().await.map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }

    async fn modify_ack_deadline(&self, subscription: &str, ack_ids: &[String], seconds: u32) -> Result<(), BackendError> {
        let response = self.request(Method::POST, "/v1/messages:modify-ack-deadline")
            .json(&DeadlineBody { subscription, ack_ids, seconds })
            .send().await.map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }

    async fn seek_to_time(&self, subscription: &str, time: DateTime<Utc>) -> Result<(), BackendError> {
        let response = self.request(Method::POST, "/v1/subscriptions:seek")
            .json(&SeekBody { subscription, time })
            .send().await.map_err(|e| BackendError::Unavailable(e.to_string()))?;
        self.empty(response).await
    }
}
