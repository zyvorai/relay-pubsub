// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::result_large_err)]

pub mod action_gateway;
pub mod auth;
pub mod backend;
pub mod config;
pub mod grpc;
pub mod http_backend;
pub mod log_buffer;
pub mod memory;
pub mod metrics;
pub mod model;
pub mod push;
pub mod relay_events_backend;
pub mod rest;
pub mod tls;

pub mod google {
    pub mod pubsub {
        pub mod v1 {
            tonic::include_proto!("google.pubsub.v1");
        }
    }
}
