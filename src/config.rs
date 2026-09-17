// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

use clap::{Parser, ValueEnum};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, ValueEnum)]
pub enum BackendKind {
    Memory,
    Http,
    /// Targets Zyvor Relay's real API (POST /v1/events + Action Gateway
    /// contract) instead of Http's invented topics/subscriptions REST
    /// contract. See docs/RELAY_EVENTS_BACKEND.md.
    RelayEvents,
    /// Scaffold for a future native gRPC Relay client (`src/grpc_backend.rs`).
    /// Compiles and starts, but data-plane calls return "not fully implemented"
    /// until a Relay gRPC proto lands in-repo. Prefer `relay-events` in production.
    Grpc,
}

/// Fixed Fasal event catalog (docs/FASAL_ACCOMMODATION.md #4.1/#4.2 in the
/// zyvor/relay repo) — topic name is the Relay event type. Used only to
/// pre-register topics at startup for admin-UI visibility; publish() forwards
/// any non-actions topic to Relay regardless of catalog membership.
pub const FASAL_CATALOG: &[&str] = &[
    "irrigation.required",
    "soil.moisture.critical",
    "fertigation.required",
    "disease.risk.critical",
    "device.control.required",
    "crop.advisory",
    "weather.advisory",
    "spray.advisory",
    "frost.alert",
    "pest.advisory",
];

/// relay-edge firewater + edge event types (industrial plant simulator).
pub const EDGE_CATALOG: &[&str] = &[
    "firewater.tank.low",
    "firewater.pressure.low",
    "firewater.demand.active",
    "firewater.pump.fail",
    "firewater.valve.closed",
    "firewater.flow.detected",
    "firewater.freeze.risk",
    "firewater.hydrant.tamper",
    "firewater.leak.acoustic",
    "firewater.pump.vibration",
    "edge.vision.fire",
    "edge.comms.down",
    "edge.power.fail",
    "edge.gas.alarm",
    "edge.control.fault",
    "edge.access.breach",
    "edge.runtime.down",
    "telemetry.sample",
];

/// relay-edge remote-edge fleet simulator (distributed site NOC).
pub const REMOTE_EDGE_CATALOG: &[&str] = &[
    "remote-edge.link.starlink.degraded",
    "remote-edge.link.offline",
    "remote-edge.galleon.thermal",
    "remote-edge.vision.intrusion",
    "remote-edge.iot.flood",
    "remote-edge.uav.rtb",
];

/// relay-edge master fleet catalog (all edge classes).
pub const FLEET_CATALOG: &[&str] = &[
    "fleet.power.island",
    "fleet.robot.lost",
    "fleet.ot.ids",
    "fleet.env.exceedance",
    "fleet.dc.thermal",
    "fleet.access.fault",
];

/// All pre-registered topic names for relay-events admin UI visibility.
pub fn relay_events_catalog() -> Vec<&'static str> {
    FASAL_CATALOG
        .iter()
        .chain(EDGE_CATALOG.iter())
        .chain(REMOTE_EDGE_CATALOG.iter())
        .chain(FLEET_CATALOG.iter())
        .copied()
        .collect()
}

fn parse_env_bool(s: &str) -> Result<bool, String> {
    match s {
        "1" | "true" | "TRUE" | "yes" | "YES" => Ok(true),
        "0" | "false" | "FALSE" | "no" | "NO" => Ok(false),
        other => Err(format!(
            "invalid boolean {other:?}, expected 0/1/true/false"
        )),
    }
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

    /// Path to a PEM certificate for the gRPC/REST listeners. Both listeners
    /// are TLS-only (HTTPS/gRPCS) — there is no plaintext mode. If this file
    /// (and `tls_key`) doesn't exist, a self-signed cert/key pair is
    /// generated once and persisted here.
    #[arg(
        long,
        env = "PUBSUB_TLS_CERT",
        default_value = "/var/lib/relay-pubsub/tls/cert.pem"
    )]
    pub tls_cert: PathBuf,

    #[arg(
        long,
        env = "PUBSUB_TLS_KEY",
        default_value = "/var/lib/relay-pubsub/tls/key.pem"
    )]
    pub tls_key: PathBuf,

    /// Hostnames/IPs to embed in the generated self-signed cert's SAN list.
    /// Only used the first time a cert is generated (see `tls_cert`) — set
    /// this to the gateway's real hostname/IP before first start if clients
    /// will validate the cert's name rather than skip verification.
    #[arg(
        long,
        env = "PUBSUB_TLS_SAN",
        value_delimiter = ',',
        default_value = "localhost,relay-pubsub"
    )]
    pub tls_san: Vec<String>,

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

    /// Only used by --backend relay-events.
    #[arg(long, env = "FASAL_GCP_PROJECT", default_value = "fasal-onprem")]
    pub fasal_gcp_project: String,
    #[arg(long, env = "FASAL_ACTIONS_TOPIC", default_value = "farm-actions")]
    pub fasal_actions_topic: String,
    #[arg(
        long,
        env = "FASAL_ACTIONS_SUBSCRIPTION",
        default_value = "farm-actions-sub"
    )]
    pub fasal_actions_subscription: String,

    /// Durable JSON state path for memory / relay-events local queues.
    #[arg(
        long,
        env = "PUBSUB_DATA_DIR",
        default_value = "/var/lib/relay-pubsub/data"
    )]
    pub data_dir: PathBuf,

    /// Enable durable persistence for memory and relay-events backends.
    #[arg(
        long,
        env = "PUBSUB_PERSIST",
        default_value = "false",
        value_parser = parse_env_bool
    )]
    pub persist: bool,

    /// Comma-separated project allowlist (`projects/foo`). Empty = all.
    #[arg(long, env = "PUBSUB_ALLOWED_PROJECTS", value_delimiter = ',')]
    pub allowed_projects: Vec<String>,

    /// Identity→project map: `sub=projects/foo,email@x=projects/bar`
    #[arg(long, env = "PUBSUB_IDENTITY_PROJECT_MAP", default_value = "")]
    pub identity_project_map: String,

    /// Push dispatcher poll interval seconds (0 disables).
    #[arg(long, env = "PUBSUB_PUSH_INTERVAL_SECONDS", default_value_t = 2)]
    pub push_interval_seconds: u64,
}
