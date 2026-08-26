use clap::{Parser, ValueEnum};
use std::net::SocketAddr;

#[derive(Debug, Clone, ValueEnum)]
pub enum BackendKind {
    Memory,
    Http,
}

#[derive(Debug, Clone, Parser)]
#[command(name = "relay-pubsub")]
#[command(about = "Google Cloud Pub/Sub compatibility gateway for Zyvor Relay")]
#[command(version)]
pub struct Config {
    #[arg(long, env = "PUBSUB_GRPC_ADDR", default_value = "0.0.0.0:50051")]
    pub grpc_addr: SocketAddr,

    #[arg(long, env = "PUBSUB_HTTP_ADDR", default_value = "0.0.0.0:8080")]
    pub http_addr: SocketAddr,

    #[arg(long, env = "RELAY_BACKEND", value_enum, default_value = "memory")]
    pub backend: BackendKind,

    #[arg(long, env = "RELAY_BASE_URL", default_value = "http://relay:9090")]
    pub relay_base_url: String,

    #[arg(long, env = "RELAY_AUTH_TOKEN")]
    pub relay_auth_token: Option<String>,

    #[arg(long, env = "RELAY_PUBSUB_AUTH_TOKEN")]
    pub gateway_auth_token: Option<String>,

    #[arg(long, env = "RELAY_HTTP_TIMEOUT_SECONDS", default_value_t = 15)]
    pub relay_http_timeout_seconds: u64,
}
