use clap::Parser;
use relay_pubsub::{
    action_gateway::{self, ActionGatewayState},
    backend::RelayBackend,
    config::{BackendKind, Config, FASAL_CATALOG},
    google::pubsub::v1::{publisher_server::PublisherServer, subscriber_server::SubscriberServer},
    grpc::GatewayService,
    http_backend::HttpRelayBackend,
    memory::MemoryBackend,
    metrics::Metrics,
    model::{SubscriptionSpec, TopicSpec},
    relay_events_backend::RelayEventsBackend,
    rest::{router, HttpState},
};
use std::collections::HashMap;
use std::{sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tonic::transport::Server;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();

    let mut config = Config::parse();
    // An EnvironmentFile/.env with `RELAY_AUTH_TOKEN=` (present but empty) is
    // indistinguishable to clap's env parsing from a real empty token, which
    // would otherwise require an unsatisfiable empty bearer header. Treat an
    // empty value the same as unset.
    config.relay_auth_token = config.relay_auth_token.filter(|s| !s.is_empty());
    config.gateway_auth_token = config.gateway_auth_token.filter(|s| !s.is_empty());
    let metrics = Metrics::new();
    let actions_topic = format!(
        "projects/{}/topics/{}",
        config.fasal_gcp_project, config.fasal_actions_topic
    );
    let actions_subscription = format!(
        "projects/{}/subscriptions/{}",
        config.fasal_gcp_project, config.fasal_actions_subscription
    );
    let is_relay_events = matches!(config.backend, BackendKind::RelayEvents);

    let backend: Arc<dyn RelayBackend> = match config.backend {
        BackendKind::Memory => {
            info!("using in-memory Relay backend (demo/test mode)");
            Arc::new(MemoryBackend::new())
        }
        BackendKind::Http => {
            info!(base_url = %config.relay_base_url, "using Zyvor Relay HTTP backend");
            Arc::new(HttpRelayBackend::new(
                config.relay_base_url.clone(),
                config.relay_auth_token.clone(),
                Duration::from_secs(config.relay_http_timeout_seconds),
            )?)
        }
        BackendKind::RelayEvents => {
            info!(base_url = %config.relay_base_url, %actions_topic, "using Zyvor Relay events backend (real /v1/events + Action Gateway)");
            let backend = RelayEventsBackend::new(
                config.relay_base_url.clone(),
                config.relay_auth_token.clone(),
                Duration::from_secs(config.relay_http_timeout_seconds),
                actions_topic.clone(),
            )?;
            // Pre-register the fixed Fasal catalog + actions topic/subscription
            // so they're visible via list_topics/list_subscriptions even
            // before the first publish/action arrives.
            for name in FASAL_CATALOG {
                let full = format!("projects/{}/topics/{name}", config.fasal_gcp_project);
                let _ = backend
                    .create_topic(TopicSpec {
                        name: full,
                        labels: HashMap::new(),
                        kms_key_name: String::new(),
                    })
                    .await;
            }
            backend
                .create_topic(TopicSpec {
                    name: actions_topic.clone(),
                    labels: HashMap::new(),
                    kms_key_name: String::new(),
                })
                .await?;
            backend
                .create_subscription(SubscriptionSpec {
                    name: actions_subscription.clone(),
                    topic: actions_topic.clone(),
                    ack_deadline_seconds: 30,
                    labels: HashMap::new(),
                    enable_message_ordering: false,
                    enable_exactly_once_delivery: false,
                    dead_letter: None,
                    retry: None,
                    push_endpoint: None,
                })
                .await?;
            Arc::new(backend)
        }
    };

    let gateway = GatewayService::new(
        backend.clone(),
        config.gateway_auth_token.clone(),
        metrics.clone(),
    );
    let http_state = HttpState {
        backend: backend.clone(),
        auth_token: config.gateway_auth_token.clone(),
        metrics: metrics.clone(),
    };
    let mut http_router = router(http_state);
    if is_relay_events {
        http_router = http_router.merge(action_gateway::router(ActionGatewayState::new(
            backend,
            actions_topic,
        )));
    }

    let grpc_addr = config.grpc_addr;
    let grpc_gateway = gateway.clone();
    let mut grpc = tokio::spawn(async move {
        info!(%grpc_addr, "Pub/Sub gRPC endpoint listening");
        Server::builder()
            .add_service(PublisherServer::new(grpc_gateway.clone()))
            .add_service(SubscriberServer::new(grpc_gateway))
            .serve(grpc_addr)
            .await
    });

    let http_addr = config.http_addr;
    let mut http = tokio::spawn(async move {
        let listener = TcpListener::bind(http_addr).await?;
        info!(%http_addr, "Pub/Sub REST/admin endpoint listening");
        axum::serve(listener, http_router).await
    });

    tokio::select! {
        result = &mut grpc => {
            http.abort();
            match result { Ok(Ok(())) => {}, Ok(Err(e)) => return Err(e.into()), Err(e) => return Err(e.into()) }
        },
        result = &mut http => {
            grpc.abort();
            match result { Ok(Ok(())) => {}, Ok(Err(e)) => return Err(e.into()), Err(e) => return Err(e.into()) }
        },
        _ = tokio::signal::ctrl_c() => {
            info!("shutdown requested");
            grpc.abort();
            http.abort();
        }
    }

    Ok(())
}
