//! Validated ModelId newtype.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ModelIdError {
    #[error("model ID cannot be empty")]
    Empty,
    #[error("model ID exceeds maximum length of 256 characters (got {0})")]
    TooLong(usize),
    #[error("model ID contains invalid characters")]
    InvalidCharacters,
}

/// An opaque, validated model identifier.
///
/// Ensures model identifiers are non-empty, within 256 characters,
/// and do not contain control characters.
#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ModelId(String);

impl ModelId {
    /// Constructs and validates a new `ModelId`.
    pub fn new(raw: &str) -> Result<Self, ModelIdError> {
        if raw.is_empty() {
            return Err(ModelIdError::Empty);
        }

        if raw.chars().any(|c| c.is_control()) {
            return Err(ModelIdError::InvalidCharacters);
        }

        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ModelIdError::Empty);
        }

        let char_count = trimmed.chars().count();
        if char_count > 256 {
            return Err(ModelIdError::TooLong(char_count));
        }

        Ok(Self(trimmed.to_string()))
    }

    /// Returns the model identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ModelId(\"{}\")", self.0)
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for ModelId {
    type Error = ModelIdError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

impl From<ModelId> for String {
    fn from(value: ModelId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_model_id() {
        let m = ModelId::new("gpt-4o").unwrap();
        assert_eq!(m.as_str(), "gpt-4o");
        assert_eq!(format!("{:?}", m), "ModelId(\"gpt-4o\")");
        assert_eq!(format!("{}", m), "gpt-4o");
    }

    #[test]
    fn test_empty_model_id() {
        assert_eq!(ModelId::new(""), Err(ModelIdError::Empty));
        assert_eq!(ModelId::new("   "), Err(ModelIdError::Empty));
    }

    #[test]
    fn test_too_long_model_id() {
        let long = "m".repeat(257);
        assert_eq!(ModelId::new(&long), Err(ModelIdError::TooLong(257)));
    }

    #[test]
    fn test_control_character_model_id() {
        assert_eq!(ModelId::new("gpt-4\0"), Err(ModelIdError::InvalidCharacters));
        assert_eq!(ModelId::new("gpt-4\n"), Err(ModelIdError::InvalidCharacters));
    }
}
