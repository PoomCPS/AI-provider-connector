//! Validated EndpointUrl newtype.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EndpointUrlError {
    #[error("endpoint URL cannot be empty")]
    Empty,
    #[error("invalid URL: {0}")]
    Parse(String),
    #[error("endpoint URL must use HTTPS, except for localhost/127.0.0.1 (got '{0}')")]
    InsecureScheme(String),
    #[error("endpoint URL cannot contain userinfo credentials")]
    UserInfoNotAllowed,
}

/// An opaque, validated HTTP(S) endpoint URL.
///
/// Enforces HTTPS for all remote endpoints while permitting plain HTTP for
/// local testing (`localhost`, `127.0.0.1`, `[::1]`). Rejects user-info credentials.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EndpointUrl(url::Url);

impl EndpointUrl {
    /// Constructs and validates a new `EndpointUrl`.
    pub fn new(raw: &str) -> Result<Self, EndpointUrlError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(EndpointUrlError::Empty);
        }

        let parsed = url::Url::parse(trimmed).map_err(|e| EndpointUrlError::Parse(e.to_string()))?;

        // Disallow userinfo (e.g. https://user:pass@example.com/)
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(EndpointUrlError::UserInfoNotAllowed);
        }

        let scheme = parsed.scheme();
        if scheme == "https" {
            Ok(Self(parsed))
        } else if scheme == "http" {
            let host_str = parsed.host_str().unwrap_or("");
            if host_str == "localhost" || host_str == "127.0.0.1" || host_str == "[::1]" || host_str == "::1" {
                Ok(Self(parsed))
            } else {
                Err(EndpointUrlError::InsecureScheme(raw.to_string()))
            }
        } else {
            Err(EndpointUrlError::InsecureScheme(raw.to_string()))
        }
    }

    /// Returns the URL as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns a reference to the parsed `url::Url`.
    pub fn url(&self) -> &url::Url {
        &self.0
    }
}

impl std::fmt::Debug for EndpointUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EndpointUrl(\"{}\")", self.0)
    }
}

impl std::fmt::Display for EndpointUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for EndpointUrl {
    type Error = EndpointUrlError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

impl From<EndpointUrl> for String {
    fn from(value: EndpointUrl) -> Self {
        value.0.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_https_url() {
        let url = EndpointUrl::new("https://api.openai.com/v1").unwrap();
        assert_eq!(url.as_str(), "https://api.openai.com/v1");
        assert_eq!(format!("{:?}", url), "EndpointUrl(\"https://api.openai.com/v1\")");
        assert_eq!(format!("{}", url), "https://api.openai.com/v1");
    }

    #[test]
    fn test_valid_localhost_http() {
        let u1 = EndpointUrl::new("http://localhost:11434/v1").unwrap();
        assert_eq!(u1.as_str(), "http://localhost:11434/v1");

        let u2 = EndpointUrl::new("http://127.0.0.1:8080").unwrap();
        assert_eq!(u2.as_str(), "http://127.0.0.1:8080/");
    }

    #[test]
    fn test_insecure_remote_http() {
        let res = EndpointUrl::new("http://api.openai.com/v1");
        assert!(matches!(res, Err(EndpointUrlError::InsecureScheme(_))));
    }

    #[test]
    fn test_user_info_rejected() {
        let res = EndpointUrl::new("https://admin:secret@api.openai.com/v1");
        assert_eq!(res, Err(EndpointUrlError::UserInfoNotAllowed));
    }

    #[test]
    fn test_empty_or_invalid_url() {
        assert_eq!(EndpointUrl::new(""), Err(EndpointUrlError::Empty));
        assert!(matches!(EndpointUrl::new("not a url"), Err(EndpointUrlError::Parse(_))));
    }
}
