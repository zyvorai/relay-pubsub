// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

//! Optional CloudEvents 1.0 attribute projection.
//!
//! When a publish payload is `application/cloudevents+json` (or the body looks
//! like a CloudEvent), context attributes are copied onto the Pub/Sub message
//! so subscription filters can match `ce-type` / `ce-source` without parsing
//! the body. Relay-native keys (`relay.topic`) are also stamped.

use crate::model::NewMessage;
use serde_json::Value;

pub fn enrich_message(topic: &str, message: &mut NewMessage) {
    let topic_leaf = topic.rsplit('/').next().unwrap_or(topic);
    message
        .attributes
        .entry("relay.topic".into())
        .or_insert_with(|| topic_leaf.to_string());

    let content_type = message
        .attributes
        .get("content-type")
        .or_else(|| message.attributes.get("Content-Type"))
        .cloned()
        .unwrap_or_default();
    let looks_like_ce = content_type.contains("cloudevents")
        || serde_json::from_slice::<Value>(&message.data)
            .ok()
            .and_then(|v| v.get("specversion").and_then(Value::as_str).map(str::to_string))
            .is_some();
    if !looks_like_ce {
        return;
    }
    let Ok(body) = serde_json::from_slice::<Value>(&message.data) else {
        return;
    };
    stamp(&mut message.attributes, "ce-id", body.get("id"));
    stamp(&mut message.attributes, "ce-source", body.get("source"));
    stamp(&mut message.attributes, "ce-type", body.get("type"));
    stamp(
        &mut message.attributes,
        "ce-specversion",
        body.get("specversion"),
    );
    stamp(&mut message.attributes, "ce-subject", body.get("subject"));
}

fn stamp(
    attributes: &mut std::collections::HashMap<String, String>,
    key: &str,
    value: Option<&Value>,
) {
    let Some(value) = value.and_then(Value::as_str) else {
        return;
    };
    attributes.entry(key.to_string()).or_insert_with(|| value.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn projects_cloud_event_context() {
        let mut msg = NewMessage {
            data: br#"{"specversion":"1.0","id":"1","source":"farm/pune","type":"irrigation.required"}"#
                .to_vec(),
            attributes: HashMap::from([("content-type".into(), "application/cloudevents+json".into())]),
            ordering_key: String::new(),
            message_id: String::new(),
        };
        enrich_message("projects/demo/topics/irrigation.required", &mut msg);
        assert_eq!(msg.attributes.get("ce-type").unwrap(), "irrigation.required");
        assert_eq!(msg.attributes.get("relay.topic").unwrap(), "irrigation.required");
    }
}
