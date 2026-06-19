use std::collections::HashMap;

#[cfg(feature = "cli")]
use std::net::IpAddr;

#[cfg(feature = "cli")]
use std::{env, fs};

#[cfg(feature = "cli")]
use std::path::PathBuf;

#[cfg(feature = "cli")]
use std::process::Command;

#[cfg(feature = "cli")]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
#[cfg(feature = "cli")]
use reqwest::Certificate;
#[cfg(feature = "cli")]
use reqwest::blocking::ClientBuilder;

#[cfg(feature = "cli")]
use reqwest::blocking::Response;

#[cfg(feature = "cli")]
use crate::error::{Error, Result as ZminResult};

#[cfg(feature = "cli")]
use serde::de::DeserializeOwned;

#[cfg(feature = "cli")]
use sha3::{Digest as ShaDigest, Sha3_256};

#[cfg(feature = "cli")]
const EMBEDDED_CA_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../security/pinning/pinned_ca.pem"
));
#[cfg(feature = "cli")]
const EMBEDDED_CA_SHA3_256: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../security/pinning/pinned_ca.sha3"
));

use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: HashMap::new(),
            body: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub trait HttpTransport: Send + Sync {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse>;
}

pub trait EventStream: Send + Sync {
    fn open(&self, url: &str, headers: &HashMap<String, String>) -> Result<StreamHandle>;
}

#[derive(Debug)]
pub struct StreamHandle;

impl StreamHandle {
    pub fn close(self) {}
}

#[cfg(feature = "cli")]
pub fn configure_blocking_client_builder(
    mut builder: ClientBuilder,
    host_hint: Option<&str>,
) -> ZminResult<ClientBuilder> {
    let mut root_certs = Vec::new();

    if let Some(cert) = load_pinned_ca()? {
        root_certs.push(cert);
    } else if let Ok(cert_path) = std::env::var("ZMIN_CA_CERT") {
        let bytes = fs::read(&cert_path).map_err(|err| Error::Config {
            details: format!("load ZMIN_CA_CERT {cert_path}: {err}"),
        })?;
        let cert = Certificate::from_pem(&bytes).map_err(|err| Error::Config {
            details: format!("parse ZMIN_CA_CERT {cert_path}: {err}"),
        })?;
        root_certs.push(cert);
    }

    if let Some(host) = host_hint
        && host_is_loopback(host)
        && let Some(cert) = load_mkcert_root()?
    {
        root_certs.push(cert);
    }

    if !root_certs.is_empty() {
        builder = builder.tls_certs_only(root_certs);
    }

    if std::env::var("ZMIN_ACCEPT_INVALID_CERTS").is_ok() {
        builder = builder.danger_accept_invalid_certs(true);
    }

    Ok(builder)
}

#[cfg(feature = "cli")]
pub fn parse_json_response<T>(response: Response, context: &str) -> ZminResult<T>
where
    T: DeserializeOwned,
{
    let status = response.status();
    let body = response.text().map_err(|err| Error::Transport {
        details: format!("read {context}: {err}"),
    })?;

    serde_json::from_str(&body).map_err(|err| {
        let trimmed = body.trim();
        let preview: String = if trimmed.is_empty() {
            "<empty>".into()
        } else {
            trimmed.chars().take(200).collect()
        };
        Error::Transport {
            details: format!("decode {context}: {err}; status={status}; body={preview}"),
        }
    })
}

#[cfg(feature = "cli")]
fn load_pinned_ca() -> ZminResult<Option<Certificate>> {
    let pem_source = if let Ok(path) = env::var("ZMIN_PINNED_CA_PEM") {
        Some(fs::read(&path).map_err(|err| Error::Config {
            details: format!("load ZMIN_PINNED_CA_PEM {path}: {err}"),
        })?)
    } else if let Some(inline) = option_env!("ZMIN_PINNED_CA_PEM_INLINE") {
        Some(inline.as_bytes().to_vec())
    } else {
        let embedded = EMBEDDED_CA_PEM.trim();
        if embedded.is_empty() {
            None
        } else {
            Some(embedded.as_bytes().to_vec())
        }
    };

    let Some(pem) = pem_source else {
        return Ok(None);
    };

    let der = pem_to_der(&pem)?;
    let cert = Certificate::from_der(&der).map_err(|err| Error::Config {
        details: format!("parse pinned CA: {err}"),
    })?;

    let target_fingerprint = env::var("ZMIN_PINNED_CA_SHA3_256")
        .ok()
        .map(|v| v.trim().to_owned())
        .or_else(|| option_env!("ZMIN_PINNED_CA_SHA3_256").map(|v| v.trim().to_owned()))
        .or_else(|| {
            let trimmed = EMBEDDED_CA_SHA3_256.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_owned())
            }
        });

    if let Some(expected) = target_fingerprint {
        verify_certificate_fingerprint(&der, &expected)?;
    } else {
        println!(
            "warning: ZMIN_PINNED_CA_PEM provided without ZMIN_PINNED_CA_SHA3_256; pinning disabled"
        );
    }

    Ok(Some(cert))
}

#[cfg(feature = "cli")]
fn verify_certificate_fingerprint(der: &[u8], expected: &str) -> ZminResult<()> {
    let mut hasher = Sha3_256::new();
    hasher.update(der);
    let computed = hex::encode_upper(hasher.finalize());
    let target = normalize_fingerprint(expected);
    if computed != target {
        return Err(Error::Config {
            details: format!(
                "pinned certificate fingerprint mismatch (expected {}, got {})",
                target, computed
            ),
        });
    }
    Ok(())
}

#[cfg(feature = "cli")]
fn load_mkcert_root() -> ZminResult<Option<Certificate>> {
    let Some(root_dir) = detect_mkcert_caroot() else {
        return Ok(None);
    };
    let pem_path = root_dir.join("rootCA.pem");
    if !pem_path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&pem_path).map_err(|err| Error::Config {
        details: format!("load mkcert root {}: {err}", pem_path.display()),
    })?;
    let cert = Certificate::from_pem(&bytes).map_err(|err| Error::Config {
        details: format!("parse mkcert root {}: {err}", pem_path.display()),
    })?;
    Ok(Some(cert))
}

#[cfg(feature = "cli")]
fn detect_mkcert_caroot() -> Option<PathBuf> {
    if let Ok(path) = env::var("ZMIN_MKCERT_CAROOT") {
        let candidate = PathBuf::from(path.trim());
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Ok(path) = env::var("CAROOT") {
        let candidate = PathBuf::from(path.trim());
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        let mac = home
            .join("Library")
            .join("Application Support")
            .join("mkcert");
        if mac.exists() {
            return Some(mac);
        }
        let linux = home.join(".local").join("share").join("mkcert");
        if linux.exists() {
            return Some(linux);
        }
    }

    if let Some(profile) = env::var_os("USERPROFILE").map(PathBuf::from) {
        let windows = profile.join("AppData").join("Local").join("mkcert");
        if windows.exists() {
            return Some(windows);
        }
    }

    if let Ok(output) = Command::new("mkcert").arg("-CAROOT").output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !path.is_empty() {
            let candidate = PathBuf::from(path);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(feature = "cli")]
fn host_is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(addr) = host.parse::<IpAddr>() {
        return addr.is_loopback();
    }
    false
}

#[cfg(feature = "cli")]
fn normalize_fingerprint(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .collect::<String>()
        .to_uppercase()
}

#[cfg(feature = "cli")]
fn pem_to_der(pem: &[u8]) -> ZminResult<Vec<u8>> {
    let pem_str = std::str::from_utf8(pem).map_err(|err| Error::Config {
        details: format!("pinned CA PEM is not valid UTF-8: {err}"),
    })?;
    let begin = "-----BEGIN CERTIFICATE-----";
    let end = "-----END CERTIFICATE-----";
    let start = pem_str
        .find(begin)
        .map(|idx| idx + begin.len())
        .ok_or_else(|| Error::Config {
            details: "pinned CA PEM missing BEGIN CERTIFICATE".into(),
        })?;
    let end_idx = pem_str.find(end).ok_or_else(|| Error::Config {
        details: "pinned CA PEM missing END CERTIFICATE".into(),
    })?;
    let body: String = pem_str[start..end_idx]
        .lines()
        .map(str::trim)
        .collect::<String>();
    BASE64.decode(body).map_err(|err| Error::Config {
        details: format!("decode pinned CA PEM: {err}"),
    })
}
