//! TLS configuration supporting system roots and injectable pinned root certificates.

use std::sync::{Arc, OnceLock};
use rustls::RootCertStore;

static CRYPTO_INIT: OnceLock<()> = OnceLock::new();

/// Installs the default `ring` crypto provider at most once per process.
pub fn ensure_crypto_provider_installed() {
    CRYPTO_INIT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TlsConfigError {
    #[error("failed to load system root certificates: {0}")]
    SystemRoots(String),
    #[error("invalid or empty PEM certificate data")]
    InvalidPem,
    #[error("TLS configuration error: {0}")]
    Configuration(String),
}

/// TLS configuration for provider adapters.
///
/// Defaults to standard system root certificates, or can be configured with
/// a pinned `RootCertStore` loaded from a PEM bundle.
#[derive(Clone)]
pub struct TlsConfig {
    pub pinned_roots: Option<Arc<RootCertStore>>,
}

impl PartialEq for TlsConfig {
    fn eq(&self, other: &Self) -> bool {
        match (&self.pinned_roots, &other.pinned_roots) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for TlsConfig {}

impl Default for TlsConfig {
    fn default() -> Self {
        Self { pinned_roots: None }
    }
}

impl std::fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsConfig")
            .field("is_pinned", &self.pinned_roots.is_some())
            .finish()
    }
}

impl TlsConfig {
    /// Creates a default `TlsConfig` using standard system roots.
    pub fn new() -> Self {
        Self::default()
    }

    /// Explicitly loads and pins system root certificates.
    pub fn system() -> Result<Self, TlsConfigError> {
        ensure_crypto_provider_installed();
        let mut roots = RootCertStore::empty();
        let native_certs = rustls_native_certs::load_native_certs();
        for cert in native_certs.certs {
            let _ = roots.add(cert);
        }
        Ok(Self {
            pinned_roots: Some(Arc::new(roots)),
        })
    }

    /// Constructs a `TlsConfig` with pinned certificates parsed from a PEM bundle.
    pub fn pinned(pem: &[u8]) -> Result<Self, TlsConfigError> {
        ensure_crypto_provider_installed();
        let mut roots = RootCertStore::empty();
        let mut cursor = std::io::Cursor::new(pem);
        let certs_res: Result<Vec<_>, _> = rustls_pemfile::certs(&mut cursor).collect();
        let certs = match certs_res {
            Ok(c) if c.is_empty() => return Err(TlsConfigError::InvalidPem),
            Ok(c) => c,
            Err(e) => {
                eprintln!("PEM parsing error: {:?}", e);
                return Err(TlsConfigError::InvalidPem);
            }
        };

        for cert in certs {
            roots
                .add(cert)
                .map_err(|e| TlsConfigError::Configuration(e.to_string()))?;
        }

        Ok(Self {
            pinned_roots: Some(Arc::new(roots)),
        })
    }

    /// Constructs a `reqwest::Client` configured with this TLS configuration.
    pub fn build_client(&self) -> Result<reqwest::Client, TlsConfigError> {
        ensure_crypto_provider_installed();

        if let Some(roots) = &self.pinned_roots {
            let client_config = rustls::ClientConfig::builder()
                .with_root_certificates((**roots).clone())
                .with_no_client_auth();

            reqwest::Client::builder()
                .use_preconfigured_tls(client_config)
                .build()
                .map_err(|e| TlsConfigError::Configuration(e.to_string()))
        } else {
            reqwest::Client::builder()
                .build()
                .map_err(|e| TlsConfigError::Configuration(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TEST_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIFazCCA1OgAwIBAgIRAIIQz7DSQONZRGPgu2OCiwAwDQYJKoZIhvcNAQELBQAw
TzELMAkGA1UEBhMCVVMxKTAnBgNVBAoTIEludGVybmV0IFNlY3VyaXR5IFJlc2Vh
cmNoIEdyb3VwMRUwEwYDVQQDEwxJU1JHIFJvb3QgWDEwHhcNMTUwNjA0MTEwNDM4
WhcNMzUwNjA0MTEwNDM4WjBPMQswCQYDVQQGEwJVUzEpMCcGA1UEChMgSW50ZXJu
ZXQgU2VjdXJpdHkgUmVzZWFyY2ggR3JvdXAxFTATBgNVBAMTDElTUkcgUm9vdCBY
MTCCAiIwDQYJKoZIhvcNAQEBBQADggIPADCCAgoCggIBAK3oJHP0FDfzm54rVygc
h77ct984kIxuPOZXoHj3dcKi/vVqbvYATyjb3miGbESTtrFj/RQSa78f0uoxmyF+
0TM8ukj13Xnfs7j/EvEhmkvBioZxaUpmZmyPfjxwv60pIgbz5MDmgK7iS4+3mX6U
A5/TR5d8mUgjU+g4rk8Kb4Mu0UlXjIB0ttov0DiNewNwIRt18jA8+o+u3dpjq+sW
T8KOEUt+zwvo/7V3LvSye0rgTBIlDHCNAymg4VMk7BPZ7hm/ELNKjD+Jo2FR3qyH
B5T0Y3HsLuJvW5iB4YlcNHlsdu87kGJ55tukmi8mxdAQ4Q7e2RCOFvu396j3x+UC
B5iPNgiV5+I3lg02dZ77DnKxHZu8A/lJBdiB3QW0KtZB6awBdpUKD9jf1b0SHzUv
KBds0pjBqAlkd25HN7rOrFleaJ1/ctaJxQZBKT5ZPt0m9STJEadao0xAH0ahmbWn
OlFuhjuefXKnEgV4We0+UXgVCwOPjdAvBbI+e0ocS3MFEvzG6uBQE3xDk3SzynTn
jh8BCNAw1FtxNrQHusEwMFxIt4I7mKZ9YIqioymCzLq9gwQbooMDQaHWBfEbwrbw
qHyGO0aoSCqI3Haadr8faqU9GY/rOPNk3sgrDQoo//fb4hVC1CLQJ13hef4Y53CI
rU7m2Ys6xt0nUW7/vGT1M0NPAgMBAAGjQjBAMA4GA1UdDwEB/wQEAwIBBjAPBgNV
HRMBAf8EBTADAQH/MB0GA1UdDgQWBBR5tFnme7bl5AFzgAiIyBpY9umbbjANBgkq
hkiG9w0BAQsFAAOCAgEAVR9YqbyyqFDQDLHYGmkgJykIrGF1XIpu+ILlaS/V9lZL
ubhzEFnTIZd+50xx+7LSYK05qAvqFyFWhfFQDlnrzuBZ6brJFe+GnY+EgPbk6ZGQ
3BebYhtF8GaV0nxvwuo77x/Py9auJ/GpsMiu/X1+mvoiBOv/2X/qkSsisRcOj/KK
NFtY2PwByVS5uCbMiogziUwthDyC3+6WVwW6LLv3xLfHTjuCvjHIInNzktHCgKQ5
ORAzI4JMPJ+GslWYHb4phowim57iaztXOoJwTdwJx4nLCgdNbOhdjsnvzqvHu7Ur
TkXWStAmzOVyyghqpZXjFaH3pO3JLF+l+/+sKAIuvtd7u+Nxe5AW0wdeRlN8NwdC
jNPElpzVmbUq4JUagEiuTDkHzsxHpFKVK7q4+63SM1N95R1NbdWhscdCb+ZAJzVc
oyi3B43njTOQ5yOf+1CceWxG1bQVs5ZufpsMljq4Ui0/1lvh+wjChP4kqKOJ2qxq
4RgqsahDYVvTH9w7jXbyLeiNdd8XM2w9U/t7y0Ff/9yi0GE44Za4rF2LN9d11TPA
mRGunUHBcnWEvgJBQl9nJEiU0Zsnvgc/ubhPgXRR4Xq37Z0j4r7g1SgEEzwxA57d
emyPxgcYxn/eR44/KJ4EBs+lVDR3veyJm+kXQ99b21/+jh5Xos1AnX5iItreGCc=
-----END CERTIFICATE-----
"#;

    #[test]
    fn test_tls_config_system() {
        let config = TlsConfig::system();
        assert!(config.is_ok());
    }

    #[test]
    fn test_tls_config_pinned_valid_pem() {
        let cfg = TlsConfig::pinned(VALID_TEST_PEM.as_bytes()).expect("should accept valid root CA PEM");
        assert!(cfg.pinned_roots.is_some());
    }

    #[test]
    fn test_tls_config_pinned_garbage() {
        let garbage = b"not a valid certificate at all";
        assert_eq!(TlsConfig::pinned(garbage), Err(TlsConfigError::InvalidPem));
    }
}
