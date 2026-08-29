// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

//! Gateway auth + tenant/project mapping.
//!
//! Production flow:
//! 1. authenticate caller (static bearer today; OIDC bearer JWT optional via JWKS URL)
//! 2. map identity to allowed Pub/Sub project prefixes
//! 3. reject operations outside the tenant's project set

use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Shared static bearer token (emulator-compatible). Empty = auth disabled.
    pub static_token: Option<String>,
    /// Optional OIDC JWKS URL for Bearer JWT validation.
    pub oidc_jwks_url: Option<String>,
    pub oidc_audience: Option<String>,
    pub oidc_issuer: Option<String>,
    /// Comma-separated `projects/<id>` allowlist. Empty = all projects allowed when authed.
    pub allowed_projects: Vec<String>,
    /// Map JWT `sub` / `email` claim → project prefix. Format: `sub=projects/foo,email=projects/bar`
    pub identity_project_map: HashMap<String, String>,
}

impl AuthConfig {
    pub fn from_env(
        static_token: Option<String>,
        allowed_projects: Vec<String>,
        identity_project_map: HashMap<String, String>,
    ) -> Self {
        Self {
            static_token,
            oidc_jwks_url: std::env::var("PUBSUB_OIDC_JWKS_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            oidc_audience: std::env::var("PUBSUB_OIDC_AUDIENCE")
                .ok()
                .filter(|s| !s.is_empty()),
            oidc_issuer: std::env::var("PUBSUB_OIDC_ISSUER")
                .ok()
                .filter(|s| !s.is_empty()),
            allowed_projects,
            identity_project_map,
        }
    }

    pub fn auth_required(&self) -> bool {
        self.static_token.is_some() || self.oidc_jwks_url.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub subject: String,
    pub allowed_projects: Vec<String>,
}

#[derive(Clone)]
pub struct Authenticator {
    config: AuthConfig,
    jwks_cache: std::sync::Arc<RwLock<Option<(Instant, Jwks)>>>,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize, Clone)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize, Clone)]
struct Jwk {
    kid: Option<String>,
    kty: String,
    n: Option<String>,
    e: Option<String>,
    #[allow(dead_code)]
    alg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: Option<String>,
    email: Option<String>,
    #[allow(dead_code)]
    aud: serde_json::Value,
    #[allow(dead_code)]
    iss: Option<String>,
    #[allow(dead_code)]
    exp: Option<i64>,
}

impl Authenticator {
    pub fn new(config: AuthConfig) -> Self {
        Self {
            config,
            jwks_cache: std::sync::Arc::new(RwLock::new(None)),
            client: reqwest::Client::new(),
        }
    }

    pub fn config(&self) -> &AuthConfig {
        &self.config
    }

    pub async fn authenticate_bearer(&self, header: Option<&str>) -> Result<AuthContext, String> {
        if !self.config.auth_required() {
            return Ok(AuthContext {
                subject: "anonymous".into(),
                allowed_projects: self.config.allowed_projects.clone(),
            });
        }
        let raw = header
            .and_then(|h| h.strip_prefix("Bearer "))
            .ok_or_else(|| "missing or invalid bearer token".to_string())?;

        if let Some(expected) = &self.config.static_token {
            if raw == expected {
                return Ok(AuthContext {
                    subject: "static-token".into(),
                    allowed_projects: self.config.allowed_projects.clone(),
                });
            }
        }

        if self.config.oidc_jwks_url.is_some() {
            return self.authenticate_jwt(raw).await;
        }

        Err("missing or invalid bearer token".into())
    }

    async fn authenticate_jwt(&self, token: &str) -> Result<AuthContext, String> {
        let header = decode_header(token).map_err(|e| format!("invalid jwt header: {e}"))?;
        let jwks = self.load_jwks().await?;
        let jwk = jwks
            .keys
            .iter()
            .find(|k| {
                header
                    .kid
                    .as_ref()
                    .map(|kid| k.kid.as_ref() == Some(kid))
                    .unwrap_or(false)
            })
            .or_else(|| jwks.keys.first())
            .ok_or_else(|| "no jwk available".to_string())?;
        if jwk.kty != "RSA" {
            return Err("only RSA JWKs are supported".into());
        }
        let n = jwk.n.as_deref().ok_or("jwk missing n")?;
        let e = jwk.e.as_deref().ok_or("jwk missing e")?;
        let key =
            DecodingKey::from_rsa_components(n, e).map_err(|err| format!("invalid jwk: {err}"))?;
        let mut validation = Validation::new(header.alg);
        if let Some(aud) = &self.config.oidc_audience {
            validation.set_audience(&[aud]);
        } else {
            validation.validate_aud = false;
        }
        if let Some(iss) = &self.config.oidc_issuer {
            validation.set_issuer(&[iss]);
        }
        let data = decode::<Claims>(token, &key, &validation)
            .map_err(|e| format!("jwt validation failed: {e}"))?;
        let subject = data
            .claims
            .email
            .or(data.claims.sub)
            .unwrap_or_else(|| "oidc".into());
        let mut projects = self.config.allowed_projects.clone();
        if let Some(mapped) = self.config.identity_project_map.get(&subject) {
            if !projects.iter().any(|p| p == mapped) {
                projects.push(mapped.clone());
            }
        }
        Ok(AuthContext {
            subject,
            allowed_projects: projects,
        })
    }

    async fn load_jwks(&self) -> Result<Jwks, String> {
        {
            let cache = self.jwks_cache.read().unwrap();
            if let Some((at, jwks)) = cache.as_ref() {
                if at.elapsed() < Duration::from_secs(300) {
                    return Ok(jwks.clone());
                }
            }
        }
        let url = self
            .config
            .oidc_jwks_url
            .as_ref()
            .ok_or_else(|| "oidc jwks url unset".to_string())?;
        let jwks: Jwks = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        *self.jwks_cache.write().unwrap() = Some((Instant::now(), jwks.clone()));
        Ok(jwks)
    }

    pub fn authorize_resource(&self, ctx: &AuthContext, resource: &str) -> Result<(), String> {
        if ctx.allowed_projects.is_empty() {
            return Ok(());
        }
        let project = project_of(resource)
            .ok_or_else(|| format!("cannot derive project from resource {resource}"))?;
        if ctx
            .allowed_projects
            .iter()
            .any(|p| p == &project || p == "*")
        {
            Ok(())
        } else {
            Err(format!("project {project} not allowed for {}", ctx.subject))
        }
    }
}

pub fn project_of(resource: &str) -> Option<String> {
    // projects/{id}/...
    let mut parts = resource.split('/');
    if parts.next()? != "projects" {
        return None;
    }
    let id = parts.next()?;
    Some(format!("projects/{id}"))
}

pub fn parse_identity_project_map(raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for part in raw.split(',').filter(|s| !s.is_empty()) {
        if let Some((k, v)) = part.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}
