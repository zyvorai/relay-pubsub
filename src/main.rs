// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::result_large_err)]

use clap::Parser;
use relay_pubsub::{
    action_gateway::{self, ActionGatewayState},
    auth::{parse_identity_project_map, AuthConfig, Authenticator},
    backend::RelayBackend,
    config::{relay_events_catalog, BackendKind, Config},
    google::pubsub::v1::{
        publisher_server::PublisherServer, schema_service_server::SchemaServiceServer,
        subscriber_server::SubscriberServer,
    },
    grpc::GatewayService,
    http_backend::HttpRelayBackend,
    log_buffer::{BufferLayer, LogBuffer},
    memory::MemoryBackend,
    metrics::Metrics,
    model::{SubscriptionSpec, TopicSpec},
    push,
    relay_events_backend::RelayEventsBackend,
    rest::{router, HttpState},
    tls::load_or_generate_self_signed,
};
use std::collections::HashMap;
use std::{sync::Arc, time::Duration};
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_buffer = LogBuffer::new(2000);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(BufferLayer::new(log_buffer.clone()))
        .init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls default crypto provider");

    let mut config = Config::parse();
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
    let persist_path = config.data_dir.join("state.json");

    let backend: Arc<dyn RelayBackend> = match config.backend {
        BackendKind::Memory => {
            info!("using in-memory Relay backend (demo/test mode)");
            if config.persist {
                info!(path = %persist_path.display(), "persistence enabled");
                Arc::new(MemoryBackend::with_persistence(&persist_path)?)
            } else {
                Arc::new(MemoryBackend::new())
            }
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
            let backend = if config.persist {
                info!(path = %persist_path.display(), "persistence enabled for actions queue");
                RelayEventsBackend::with_persistence(
                    &persist_path,
                    config.relay_base_url.clone(),
                    config.relay_auth_token.clone(),
                    Duration::from_secs(config.relay_http_timeout_seconds),
                    actions_topic.clone(),
                )?
            } else {
                RelayEventsBackend::new(
                    config.relay_base_url.clone(),
                    config.relay_auth_token.clone(),
                    Duration::from_secs(config.relay_http_timeout_seconds),
                    actions_topic.clone(),
                )?
            };
            for name in relay_events_catalog() {
                let full = format!("projects/{}/topics/{name}", config.fasal_gcp_project);
                let _ = backend
                    .create_topic(TopicSpec {
                        name: full,
                        labels: HashMap::new(),
                        kms_key_name: String::new(),
                    })
                    .await;
            }
            let _ = backend
                .create_topic(TopicSpec {
                    name: actions_topic.clone(),
                    labels: HashMap::new(),
                    kms_key_name: String::new(),
                })
                .await;
            let _ = backend
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
                    push_attributes: HashMap::new(),
                })
                .await;
            Arc::new(backend)
        }
    };

    let auth_config = AuthConfig::from_env(
        config.gateway_auth_token.clone(),
        config.allowed_projects.clone(),
        parse_identity_project_map(&config.identity_project_map),
    );
    let authenticator = if auth_config.auth_required() || !auth_config.allowed_projects.is_empty() {
        Some(Authenticator::new(auth_config))
    } else {
        None
    };

    let gateway = GatewayService::new(backend.clone(), authenticator.clone(), metrics.clone());
    let http_state = HttpState {
        backend: backend.clone(),
        authenticator,
        metrics: metrics.clone(),
        check_relay_ready: is_relay_events,
        relay_base_url: config.relay_base_url.clone(),
        relay_token: config.relay_auth_token.clone(),
        logs: log_buffer,
    };
    let mut http_router = router(http_state);
    if is_relay_events {
        http_router = http_router.merge(action_gateway::router(ActionGatewayState::new(
            backend.clone(),
            actions_topic,
        )));
    }

    if config.push_interval_seconds > 0 {
        push::spawn(
            backend.clone(),
            metrics.clone(),
            Duration::from_secs(config.push_interval_seconds),
        );
        info!(
            interval_s = config.push_interval_seconds,
            "push dispatcher started"
        );
    }

    let tls =
        load_or_generate_self_signed(&config.tls_cert, &config.tls_key, config.tls_san.clone())?;

    let grpc_addr = config.grpc_addr;
    let grpc_identity = Identity::from_pem(tls.cert_pem.clone(), tls.key_pem.clone());
    let grpc_gateway = gateway.clone();
    let mut grpc = tokio::spawn(async move {
        info!(%grpc_addr, "Pub/Sub gRPCS endpoint listening");
        Server::builder()
            .tls_config(ServerTlsConfig::new().identity(grpc_identity))?
            .add_service(PublisherServer::new(grpc_gateway.clone()))
            .add_service(SubscriberServer::new(grpc_gateway.clone()))
            .add_service(SchemaServiceServer::new(grpc_gateway))
            .serve(grpc_addr)
            .await
    });

    let http_addr = config.http_addr;
    let http_tls_config =
        axum_server::tls_rustls::RustlsConfig::from_pem(tls.cert_pem, tls.key_pem).await?;
    let mut http = tokio::spawn(async move {
        info!(%http_addr, "Pub/Sub REST/admin endpoint listening (HTTPS)");
        axum_server::bind_rustls(http_addr, http_tls_config)
            .serve(http_router.into_make_service())
            .await
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
