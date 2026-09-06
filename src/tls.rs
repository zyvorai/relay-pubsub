// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

//! Self-signed TLS material for a self-serving gateway (no reverse proxy in
//! front). A cert/key is generated once and persisted on disk so restarts
//! keep serving the same certificate rather than one clients must re-trust
//! every time.

use std::path::Path;

pub struct TlsMaterial {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

pub fn load_or_generate_self_signed(
    cert_path: &Path,
    key_path: &Path,
    subject_alt_names: Vec<String>,
) -> std::io::Result<TlsMaterial> {
    if cert_path.exists() && key_path.exists() {
        return Ok(TlsMaterial {
            cert_pem: std::fs::read(cert_path)?,
            key_pem: std::fs::read(key_path)?,
        });
    }

    let cert_key = rcgen::generate_simple_self_signed(subject_alt_names)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let cert_pem = cert_key.cert.pem().into_bytes();
    let key_pem = cert_key.key_pair.serialize_pem().into_bytes();

    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(cert_path, &cert_pem)?;
    std::fs::write(key_path, &key_pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(TlsMaterial { cert_pem, key_pem })
}
