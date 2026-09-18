// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

//! In-process ring buffer of tracing events for the ops console.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

const DEFAULT_CAPACITY: usize = 2000;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: u64,
    pub ts: DateTime<Utc>,
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct LogBuffer {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    next_id: u64,
    entries: VecDeque<LogEntry>,
    capacity: usize,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                next_id: 1,
                entries: VecDeque::with_capacity(capacity.min(DEFAULT_CAPACITY)),
                capacity: capacity.max(100),
            })),
        }
    }

    pub fn push(&self, level: Level, target: &str, message: impl Into<String>) {
        let mut guard = self.inner.lock().expect("log buffer lock");
        let id = guard.next_id;
        guard.next_id = guard.next_id.saturating_add(1);
        guard.entries.push_back(LogEntry {
            id,
            ts: Utc::now(),
            level: level_str(level).into(),
            target: target.to_string(),
            message: message.into(),
        });
        while guard.entries.len() > guard.capacity {
            guard.entries.pop_front();
        }
    }

    /// Entries with `id > after_id`, newest last, capped at `limit`.
    pub fn since(&self, after_id: u64, limit: usize) -> Vec<LogEntry> {
        let guard = self.inner.lock().expect("log buffer lock");
        let limit = limit.clamp(1, 2000);
        guard
            .entries
            .iter()
            .filter(|e| e.id > after_id)
            .cloned()
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn recent(&self, limit: usize) -> Vec<LogEntry> {
        let guard = self.inner.lock().expect("log buffer lock");
        let limit = limit.clamp(1, 2000);
        guard
            .entries
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

fn level_str(level: Level) -> &'static str {
    match level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARN",
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
}

pub struct BufferLayer {
    buffer: LogBuffer,
}

impl BufferLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

struct MessageVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}").trim_matches('"').to_string();
        } else {
            self.fields
                .push((field.name().to_string(), format!("{value:?}")));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }
}

impl<S> Layer<S> for BufferLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let target = meta.target();
        // Skip high-frequency access logs; keep operational events readable in the UX.
        if target.starts_with("tower_http") || target.starts_with("hyper") {
            return;
        }
        let mut visitor = MessageVisitor {
            message: String::new(),
            fields: Vec::new(),
        };
        event.record(&mut visitor);
        let mut message = visitor.message;
        if !visitor.fields.is_empty() {
            let extras = visitor
                .fields
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ");
            if message.is_empty() {
                message = extras;
            } else {
                message = format!("{message} {extras}");
            }
        }
        if message.is_empty() {
            message = meta.name().to_string();
        }
        self.buffer.push(*meta.level(), target, message);
    }
}
