//! Enrollment and the agent's on-disk identity.
//!
//! The private key is generated here and never leaves the machine; the panel
//! only sees a certificate request.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

const KEY_FILE: &str = "key.pem";
const CERT_FILE: &str = "cert.pem";
const CA_FILE: &str = "ca.pem";
const META_FILE: &str = "agent.json";

/// Where and how to reach the panel, written at enrollment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMeta {
    pub server_id: String,
    pub panel_url: String,
    pub grpc_url: String,
    pub tls_name: String,
}

/// Everything needed to open the mTLS session.
#[derive(Clone)]
pub struct Identity {
    pub meta: AgentMeta,
    pub key_pem: String,
    pub cert_pem: String,
    pub ca_pem: String,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("meta", &self.meta)
            .field("key_pem", &"[redacted]")
            .finish()
    }
}

impl Identity {
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let read = |name: &str| -> anyhow::Result<String> {
            let path = data_dir.join(name);
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
        };
        let meta: AgentMeta =
            serde_json::from_str(&read(META_FILE)?).context("parse agent.json")?;
        Ok(Self {
            meta,
            key_pem: read(KEY_FILE)?,
            cert_pem: read(CERT_FILE)?,
            ca_pem: read(CA_FILE)?,
        })
    }

    pub fn save(&self, data_dir: &Path) -> anyhow::Result<()> {
        create_private_dir(data_dir)?;
        write_private(&data_dir.join(KEY_FILE), self.key_pem.as_bytes())?;
        write_private(&data_dir.join(CERT_FILE), self.cert_pem.as_bytes())?;
        write_private(&data_dir.join(CA_FILE), self.ca_pem.as_bytes())?;
        let meta = serde_json::to_vec_pretty(&self.meta)?;
        write_private(&data_dir.join(META_FILE), &meta)
    }
}

#[derive(Serialize)]
struct EnrollRequest<'a> {
    join_token: &'a str,
    csr_pem: &'a str,
    hostname: &'a str,
}

#[derive(Deserialize)]
struct EnrollResponse {
    server_id: String,
    client_cert_pem: String,
    ca_pem: String,
    grpc_url: String,
    tls_name: String,
}

#[derive(Deserialize)]
struct ApiError {
    error: ApiErrorBody,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    message: String,
}

/// Exchange a join token for a client certificate and save the identity.
///
/// The panel URL must use `https://`; plain `http://` is accepted only for a
/// loopback address, because the reply carries the CA the agent trusts from
/// then on. When `ca_sha256` is given, the returned CA must have that SHA-256
/// fingerprint (of its DER encoding) or nothing is saved.
pub async fn enroll(
    panel_url: &str,
    token: &str,
    ca_sha256: Option<&str>,
    data_dir: &Path,
) -> anyhow::Result<Identity> {
    let panel_url = panel_url.trim_end_matches('/');
    check_panel_url(panel_url)?;
    let ca_sha256 = ca_sha256.map(parse_fingerprint).transpose()?;
    let key = rcgen::KeyPair::generate().context("generate key")?;
    let csr = rcgen::CertificateParams::new(Vec::<String>::new())
        .context("certificate request")?
        .serialize_request(&key)
        .context("sign certificate request")?
        .pem()
        .context("encode certificate request")?;
    let hostname = crate::host::hostname();
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        // /mcp/api/v1 is the machine-client prefix: it is routed to the API
        // and stays reachable when Cloudflare Access protects the panel and
        // the operator allowed agents through.
        .post(format!("{panel_url}/mcp/api/v1/agent/enroll"))
        .json(&EnrollRequest {
            join_token: token.trim(),
            csr_pem: &csr,
            hostname: &hostname,
        })
        .send()
        .await
        .with_context(|| format!("reach {panel_url}"))?;
    let status = response.status();
    if !status.is_success() {
        let message = response
            .json::<ApiError>()
            .await
            .map(|body| body.error.message)
            .unwrap_or_else(|_| status.to_string());
        anyhow::bail!("enrollment rejected: {message}");
    }
    let body: EnrollResponse = response.json().await.context("parse enrollment reply")?;
    if !body.grpc_url.starts_with("https://") {
        anyhow::bail!("the panel returned a non-TLS agent address");
    }
    if let Some(expected) = ca_sha256 {
        let actual = pem_fingerprint(&body.ca_pem)
            .ok_or_else(|| anyhow::anyhow!("the panel returned an unreadable CA certificate"))?;
        if actual != expected {
            anyhow::bail!(
                "the panel's agent CA has fingerprint {actual}, not the expected {expected}; refusing to enroll"
            );
        }
    }
    let identity = Identity {
        meta: AgentMeta {
            server_id: body.server_id,
            panel_url: panel_url.to_owned(),
            grpc_url: body.grpc_url,
            tls_name: body.tls_name,
        },
        key_pem: key.serialize_pem(),
        cert_pem: body.client_cert_pem,
        ca_pem: body.ca_pem,
    };
    identity.save(data_dir)?;
    Ok(identity)
}

/// Allow `https://`, or `http://` only to this machine.
fn check_panel_url(panel_url: &str) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(panel_url)
        .with_context(|| format!("--server {panel_url} is not a valid URL"))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if is_loopback(&url) => Ok(()),
        "http" => anyhow::bail!(
            "--server must use https://; plain http:// is allowed only for localhost, \
             because enrollment receives the CA certificate the agent trusts"
        ),
        _ => anyhow::bail!("--server must start with https://"),
    }
}

fn is_loopback(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn parse_fingerprint(value: &str) -> anyhow::Result<String> {
    let hex: String = value
        .trim()
        .trim_start_matches("sha256:")
        .chars()
        .filter(|c| *c != ':')
        .collect::<String>()
        .to_ascii_lowercase();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        anyhow::bail!("--ca-sha256 must be 64 hexadecimal characters");
    }
    Ok(hex)
}

/// SHA-256 (lowercase hex) of the DER encoding of the first certificate in `pem`.
fn pem_fingerprint(pem: &str) -> Option<String> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    use sha2::Digest;
    let der = CertificateDer::from_pem_slice(pem.as_bytes()).ok()?;
    Some(hex::encode(sha2::Sha256::digest(der.as_ref())))
}

pub fn create_private_dir(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write a file readable only by the agent, replacing it atomically.
pub fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let tmp: PathBuf = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .with_context(|| format!("write {}", tmp.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_url_must_be_https_unless_loopback() {
        for ok in [
            "https://panel.example.com",
            "https://10.0.0.5:3000",
            "http://localhost:3000",
            "http://127.0.0.1:3000",
            "http://127.1.2.3",
            "http://[::1]:3000",
        ] {
            assert!(check_panel_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://panel.example.com",
            "http://10.0.0.5:3000",
            "http://localhost.example.com",
            "http://127.0.0.1.nip.io",
            "http://[::ffff:10.0.0.1]",
            "ftp://panel.example.com",
            "panel.example.com",
        ] {
            assert!(check_panel_url(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn plain_http_is_rejected_before_any_request() {
        let dir = std::env::temp_dir().join("rudder-agent-enroll-http-test");
        let error = enroll("http://192.0.2.1:9", "join_x", None, &dir)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("https://"), "{error:#}");
        assert!(!dir.exists());
    }

    #[test]
    fn fingerprints_are_normalized_and_checked() {
        let hex = "AB".repeat(32);
        assert_eq!(parse_fingerprint(&hex).unwrap(), "ab".repeat(32));
        let colons = vec!["ab"; 32].join(":");
        assert_eq!(
            parse_fingerprint(&format!("sha256:{colons}")).unwrap(),
            "ab".repeat(32)
        );
        assert!(parse_fingerprint("abc").is_err());
        assert!(parse_fingerprint(&"zz".repeat(32)).is_err());
    }

    #[test]
    fn pem_fingerprint_hashes_the_der() {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(Vec::<String>::new())
            .unwrap()
            .self_signed(&key)
            .unwrap();
        use sha2::Digest;
        let expected = hex::encode(sha2::Sha256::digest(cert.der()));
        assert_eq!(pem_fingerprint(&cert.pem()), Some(expected));
        assert_eq!(pem_fingerprint("not a certificate"), None);
    }
}
