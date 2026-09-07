// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

//! Lightweight publish-time schema enforcement.
//!
//! This is a compatibility subset, not a full Avro/Protobuf compiler:
//! - `JSON` encoding requires a JSON payload
//! - JSON Schema-shaped definitions enforce `required` object keys
//! - Avro definitions must be JSON with a `type` field
//! - Protocol Buffer definitions must mention `syntax` or `message`

use crate::backend::BackendError;
use crate::model::SchemaSpec;
use serde_json::Value;

pub fn validate_payload(
    schema: &SchemaSpec,
    encoding: &str,
    payload: &[u8],
) -> Result<(), BackendError> {
    if payload.is_empty() {
        return Err(BackendError::InvalidArgument(
            "message body is required for a schema-bound topic".into(),
        ));
    }
    let encoding = encoding.to_ascii_uppercase();
    match encoding.as_str() {
        "" | "JSON" | "ENCODING_UNSPECIFIED" => validate_json(schema, payload),
        "BINARY" => {
            // Binary payloads are accepted as long as the schema itself is valid.
            validate_definition(schema)
        }
        other => Err(BackendError::InvalidArgument(format!(
            "unsupported schema encoding {other}"
        ))),
    }
}

pub fn validate_definition(schema: &SchemaSpec) -> Result<(), BackendError> {
    if schema.definition.trim().is_empty() {
        return Err(BackendError::InvalidArgument(
            "schema definition is required".into(),
        ));
    }
    match schema.schema_type.to_ascii_uppercase().as_str() {
        "AVRO" => {
            let value: Value = serde_json::from_str(&schema.definition).map_err(|e| {
                BackendError::InvalidArgument(format!("Avro schema is not JSON: {e}"))
            })?;
            if value.get("type").is_none() {
                return Err(BackendError::InvalidArgument(
                    "Avro schema must include a type field".into(),
                ));
            }
            Ok(())
        }
        "PROTOCOL_BUFFER" => {
            let def = schema.definition.to_ascii_lowercase();
            if def.contains("message") || def.contains("syntax") {
                Ok(())
            } else {
                Err(BackendError::InvalidArgument(
                    "Protocol Buffer schema must contain a message or syntax declaration".into(),
                ))
            }
        }
        "UNSPECIFIED" | "" | "JSON" | "JSON_SCHEMA" => {
            if schema.definition.trim_start().starts_with('{') {
                let _: Value = serde_json::from_str(&schema.definition).map_err(|e| {
                    BackendError::InvalidArgument(format!("schema definition is not JSON: {e}"))
                })?;
            }
            Ok(())
        }
        other => Err(BackendError::InvalidArgument(format!(
            "unsupported schema type {other}"
        ))),
    }
}

fn validate_json(schema: &SchemaSpec, payload: &[u8]) -> Result<(), BackendError> {
    validate_definition(schema)?;
    let body: Value = serde_json::from_slice(payload).map_err(|e| {
        BackendError::InvalidArgument(format!("JSON encoding requires a JSON payload: {e}"))
    })?;
    let Ok(def) = serde_json::from_str::<Value>(&schema.definition) else {
        return Ok(());
    };
    enforce_required(&def, &body)
}

fn enforce_required(schema: &Value, body: &Value) -> Result<(), BackendError> {
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        let object = body.as_object().ok_or_else(|| {
            BackendError::InvalidArgument("JSON schema expected an object payload".into())
        })?;
        for key in required {
            let Some(name) = key.as_str() else {
                continue;
            };
            if !object.contains_key(name) {
                return Err(BackendError::InvalidArgument(format!(
                    "payload missing required field '{name}'"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(kind: &str, def: &str) -> SchemaSpec {
        SchemaSpec {
            name: "projects/demo/schemas/event".into(),
            schema_type: kind.into(),
            definition: def.into(),
        }
    }

    #[test]
    fn json_required_fields() {
        let s = schema(
            "UNSPECIFIED",
            r#"{"type":"object","required":["site","zone"]}"#,
        );
        validate_payload(&s, "JSON", br#"{"site":"pune","zone":"north"}"#).unwrap();
        assert!(validate_payload(&s, "JSON", br#"{"site":"pune"}"#).is_err());
        assert!(validate_payload(&s, "JSON", b"not-json").is_err());
    }

    #[test]
    fn avro_definition_must_have_type() {
        let s = schema("AVRO", r#"{"type":"record","name":"Evt"}"#);
        validate_definition(&s).unwrap();
        let bad = schema("AVRO", r#"{"name":"Evt"}"#);
        assert!(validate_definition(&bad).is_err());
    }
}
