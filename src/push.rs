// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

//! Background push dispatcher: polls push subscriptions and POSTs deliveries
//! to configured push endpoints (Google Pub/Sub push compatibility subset).

use crate::backend::RelayBackend;
use crate::metrics::Metrics;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

pub fn spawn(backend: Arc<dyn RelayBackend>, metrics: Metrics, interval: Duration) {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .danger_accept_invalid_certs(
                std::env::var("PUBSUB_PUSH_TLS_INSECURE")
                    .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
                    .unwrap_or(false),
            )
            .build()
            .ok();
        let Some(client) = client else {
            warn!("push dispatcher disabled: failed to build HTTP client");
            return;
        };
        loop {
            if let Err(e) = tick(&*backend, &client, &metrics).await {
                debug!(error = %e, "push dispatcher tick failed");
            }
            tokio::time::sleep(interval).await;
        }
    });
}

async fn tick(
    backend: &dyn RelayBackend,
    client: &reqwest::Client,
    metrics: &Metrics,
) -> Result<(), String> {
    let subs = backend
        .list_push_subscriptions()
        .await
        .map_err(|e| e.to_string())?;
    for sub in subs {
        let Some(endpoint) = &sub.push_endpoint else {
            continue;
        };
        let deliveries = backend
            .pull(&sub.name, 20)
            .await
            .map_err(|e| e.to_string())?;
        for delivery in deliveries {
            let body = json!({
                "message": {
                    "data": BASE64.encode(&delivery.message.data),
                    "attributes": delivery.message.attributes,
                    "messageId": delivery.message.id,
                    "publishTime": delivery.message.published_at.to_rfc3339(),
                    "orderingKey": delivery.message.ordering_key,
                },
                "subscription": sub.name,
            });
            match client.post(endpoint).json(&body).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let _ = backend
                        .acknowledge(&sub.name, std::slice::from_ref(&delivery.ack_id))
                        .await;
                    metrics
                        .messages
                        .with_label_values(&["push_delivered"])
                        .inc();
                    info!(
                        subscription = %sub.name,
                        endpoint = %endpoint,
                        message_id = %delivery.message.id,
                        "push delivered"
                    );
                }
                Ok(resp) => {
                    warn!(
                        status = %resp.status(),
                        subscription = %sub.name,
                        "push endpoint rejected message; will retry after ack deadline"
                    );
                    metrics.messages.with_label_values(&["push_failed"]).inc();
                }
                Err(e) => {
                    warn!(error = %e, subscription = %sub.name, "push POST failed");
                    metrics.messages.with_label_values(&["push_failed"]).inc();
                }
            }
        }
    }
    Ok(())
}
