//! Validated API key newtype.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ApiKeyError {
    #[error("API key cannot be empty")]
    Empty,
    #[error("API key exceeds maximum length of 4096 bytes (got {0} bytes)")]
    TooLong(usize),
    #[error("API key contains invalid control character at byte offset {0}")]
    ControlCharacter(usize),
    #[error("API key contains invalid character for HTTP header at byte offset {0}")]
    InvalidHeaderCharacter(usize),
}

/// An opaque, validated API key.
///
/// Ensures credentials cannot contain control characters, exceed 4096 bytes,
/// or produce invalid HTTP headers. Does NOT implement Display, Serialize,
/// or full-value Debug.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Constructs and validates a new `ApiKey`.
    pub fn new(raw: &str) -> Result<Self, ApiKeyError> {
        if raw.is_empty() {
            return Err(ApiKeyError::Empty);
        }

        if raw.len() > 4096 {
            return Err(ApiKeyError::TooLong(raw.len()));
        }

        for (i, b) in raw.bytes().enumerate() {
            // Reject control characters (0x00..=0x1F and 0x7F)
            if b < 0x20 || b == 0x7F {
                return Err(ApiKeyError::ControlCharacter(i));
            }

            // HTTP header values must be printable ASCII (0x20..=0x7E)
            if !(0x20..=0x7E).contains(&b) {
                return Err(ApiKeyError::InvalidHeaderCharacter(i));
            }
        }

        Ok(Self(raw.to_string()))
    }

    /// Exposes the underlying secret string for internal provider adapter use.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// Returns a masked representation suitable for UI display (e.g., `***1234`).
    pub fn masked(&self) -> String {
        if self.0.len() <= 4 {
            "***".to_string()
        } else {
            let last_four = &self.0[self.0.len() - 4..];
            format!("***{}", last_four)
        }
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ApiKey(\"***\")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_api_key() {
        let key = ApiKey::new("sk-ant-api03-12345678").unwrap();
        assert_eq!(key.expose_secret(), "sk-ant-api03-12345678");
        assert_eq!(format!("{:?}", key), "ApiKey(\"***\")");
        assert_eq!(key.masked(), "***5678");
    }

    #[test]
    fn test_empty_api_key() {
        assert_eq!(ApiKey::new(""), Err(ApiKeyError::Empty));
    }

    #[test]
    fn test_too_long_api_key() {
        let long = "a".repeat(4097);
        assert_eq!(ApiKey::new(&long), Err(ApiKeyError::TooLong(4097)));
    }

    #[test]
    fn test_control_character() {
        assert_eq!(ApiKey::new("sk-\0key"), Err(ApiKeyError::ControlCharacter(3)));
        assert_eq!(ApiKey::new("sk-\nkey"), Err(ApiKeyError::ControlCharacter(3)));
        assert_eq!(ApiKey::new("sk-\rkey"), Err(ApiKeyError::ControlCharacter(3)));
        assert_eq!(ApiKey::new("sk-\tkey"), Err(ApiKeyError::ControlCharacter(3)));
    }

    #[test]
    fn test_non_ascii_header_character() {
        assert_eq!(ApiKey::new("sk-key-ñ"), Err(ApiKeyError::InvalidHeaderCharacter(7)));
    }
}
