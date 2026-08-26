// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};

#[derive(Clone)]
pub struct Metrics {
    registry: Registry,
    pub requests: IntCounterVec,
    pub messages: IntCounterVec,
    pub latency: HistogramVec,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();
        let requests = IntCounterVec::new(
            Opts::new(
                "relay_pubsub_requests_total",
                "Gateway requests by operation and result",
            ),
            &["transport", "operation", "result"],
        )
        .expect("valid request metric");
        let messages = IntCounterVec::new(
            Opts::new(
                "relay_pubsub_messages_total",
                "Messages published or delivered",
            ),
            &["direction"],
        )
        .expect("valid message metric");
        let latency = HistogramVec::new(
            HistogramOpts::new(
                "relay_pubsub_request_duration_seconds",
                "Gateway request latency",
            ),
            &["transport", "operation"],
        )
        .expect("valid latency metric");
        registry
            .register(Box::new(requests.clone()))
            .expect("register requests");
        registry
            .register(Box::new(messages.clone()))
            .expect("register messages");
        registry
            .register(Box::new(latency.clone()))
            .expect("register latency");
        Self {
            registry,
            requests,
            messages,
            latency,
        }
    }

    pub fn render(&self) -> String {
        let encoder = TextEncoder::new();
        let families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&families, &mut buffer)
            .expect("encode metrics");
        String::from_utf8(buffer).expect("prometheus output is utf8")
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
