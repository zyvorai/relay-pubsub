// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

//! Google Pub/Sub subscription filter subset.
//!
//! Supported expressions (attribute-only, matching Cloud Pub/Sub):
//!
//! - `attributes:name` / `NOT attributes:name`
//! - `attributes.name = "value"` / `attributes.name != "value"`
//! - `hasPrefix(attributes.name, "pre")` / `NOT hasPrefix(...)`
//! - `AND` / `OR` and parentheses
//!
//! Non-matching messages are auto-acknowledged by the pull path (Google
//! semantics). An empty filter matches every message.

use crate::backend::BackendError;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    HasKey(String),
    Eq(String, String),
    Ne(String, String),
    HasPrefix(String, String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeFilter {
    raw: String,
    expr: Option<Expr>,
}

impl AttributeFilter {
    pub fn parse(raw: &str) -> Result<Self, BackendError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Self {
                raw: String::new(),
                expr: None,
            });
        }
        let mut parser = Parser::new(trimmed);
        let expr = parser.parse_or()?;
        parser.skip_ws();
        if parser.pos != parser.src.len() {
            return Err(BackendError::InvalidArgument(format!(
                "trailing characters in filter at position {}",
                parser.pos
            )));
        }
        Ok(Self {
            raw: trimmed.to_string(),
            expr: Some(expr),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.expr.is_none()
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn matches(&self, attributes: &HashMap<String, String>) -> bool {
        match &self.expr {
            None => true,
            Some(expr) => eval(expr, attributes),
        }
    }
}

fn eval(expr: &Expr, attributes: &HashMap<String, String>) -> bool {
    match expr {
        Expr::HasKey(key) => attributes.contains_key(key),
        Expr::Eq(key, value) => attributes.get(key).is_some_and(|v| v == value),
        Expr::Ne(key, value) => !attributes.get(key).is_some_and(|v| v == value),
        Expr::HasPrefix(key, prefix) => attributes
            .get(key)
            .is_some_and(|v| v.starts_with(prefix.as_str())),
        Expr::Not(inner) => !eval(inner, attributes),
        Expr::And(left, right) => eval(left, attributes) && eval(right, attributes),
        Expr::Or(left, right) => eval(left, attributes) || eval(right, attributes),
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s)
    }

    fn eat(&mut self, s: &str) -> bool {
        self.skip_ws();
        if self.starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, s: &str) -> Result<(), BackendError> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(BackendError::InvalidArgument(format!(
                "expected '{s}' in filter at position {}",
                self.pos
            )))
        }
    }

    fn parse_or(&mut self) -> Result<Expr, BackendError> {
        let mut left = self.parse_and()?;
        loop {
            self.skip_ws();
            if self.eat("OR") {
                let right = self.parse_and()?;
                left = Expr::Or(Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, BackendError> {
        let mut left = self.parse_not()?;
        loop {
            self.skip_ws();
            if self.eat("AND") {
                let right = self.parse_not()?;
                left = Expr::And(Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, BackendError> {
        self.skip_ws();
        if self.eat("NOT") {
            Ok(Expr::Not(Box::new(self.parse_not()?)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, BackendError> {
        self.skip_ws();
        if self.eat("(") {
            let inner = self.parse_or()?;
            self.expect(")")?;
            return Ok(inner);
        }
        if self.eat("hasPrefix") {
            self.expect("(")?;
            let key = self.parse_attr_field()?;
            self.expect(",")?;
            let prefix = self.parse_string()?;
            self.expect(")")?;
            return Ok(Expr::HasPrefix(key, prefix));
        }
        if self.eat("attributes:") {
            let key = self.parse_ident()?;
            return Ok(Expr::HasKey(key));
        }
        if self.starts_with("attributes.") {
            let key = self.parse_attr_field()?;
            self.skip_ws();
            if self.eat("!=") {
                let value = self.parse_string()?;
                return Ok(Expr::Ne(key, value));
            }
            if self.eat("=") {
                let value = self.parse_string()?;
                return Ok(Expr::Eq(key, value));
            }
            return Err(BackendError::InvalidArgument(format!(
                "expected comparison after attributes.{key}"
            )));
        }
        Err(BackendError::InvalidArgument(format!(
            "unexpected token in filter at position {}",
            self.pos
        )))
    }

    fn parse_attr_field(&mut self) -> Result<String, BackendError> {
        self.skip_ws();
        self.expect("attributes.")?;
        self.parse_ident()
    }

    fn parse_ident(&mut self) -> Result<String, BackendError> {
        self.skip_ws();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                self.bump();
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(BackendError::InvalidArgument(
                "expected attribute name in filter".into(),
            ));
        }
        Ok(self.src[start..self.pos].to_string())
    }

    fn parse_string(&mut self) -> Result<String, BackendError> {
        self.skip_ws();
        if !self.eat("\"") {
            return Err(BackendError::InvalidArgument(
                "expected quoted string in filter".into(),
            ));
        }
        let mut out = String::new();
        while let Some(c) = self.bump() {
            match c {
                '"' => return Ok(out),
                '\\' => match self.bump() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => break,
                },
                other => out.push(other),
            }
        }
        Err(BackendError::InvalidArgument(
            "unterminated string in filter".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn empty_matches_all() {
        let f = AttributeFilter::parse("").unwrap();
        assert!(f.matches(&attrs(&[("site", "pune")])));
    }

    #[test]
    fn has_key_and_eq() {
        let f = AttributeFilter::parse(r#"attributes:site AND attributes.zone = "north""#).unwrap();
        assert!(f.matches(&attrs(&[("site", "x"), ("zone", "north")])));
        assert!(!f.matches(&attrs(&[("zone", "north")])));
        assert!(!f.matches(&attrs(&[("site", "x"), ("zone", "south")])));
    }

    #[test]
    fn prefix_or_not() {
        let f = AttributeFilter::parse(
            r#"hasPrefix(attributes.device, "pump-") OR NOT attributes:device"#,
        )
        .unwrap();
        assert!(f.matches(&attrs(&[("device", "pump-1")])));
        assert!(f.matches(&attrs(&[])));
        assert!(!f.matches(&attrs(&[("device", "valve-1")])));
    }

    #[test]
    fn rejects_junk() {
        assert!(AttributeFilter::parse("payload.foo = 1").is_err());
    }
}
