//! Supported AI provider kinds.

use crate::types::EndpointUrl;

/// Enumeration of supported AI provider families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[serde(rename = "openai")]
    OpenAICompatible,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "gemini")]
    Gemini,
}

impl ProviderKind {
    /// Returns the provider identifier as a static string slice.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAICompatible => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    /// Returns the default official endpoint URL for this provider.
    pub fn default_endpoint(&self) -> EndpointUrl {
        match self {
            Self::OpenAICompatible => EndpointUrl::new("https://api.openai.com/v1").expect("valid url"),
            Self::Anthropic => EndpointUrl::new("https://api.anthropic.com/v1").expect("valid url"),
            Self::Gemini => EndpointUrl::new("https://generativelanguage.googleapis.com/v1beta").expect("valid url"),
        }
    }

    /// Returns all known provider kinds.
    pub fn all() -> &'static [ProviderKind] {
        &[Self::OpenAICompatible, Self::Anthropic, Self::Gemini]
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" | "openai-compatible" | "openaicompatible" => Ok(Self::OpenAICompatible),
            "anthropic" => Ok(Self::Anthropic),
            "gemini" => Ok(Self::Gemini),
            other => Err(format!("Unknown provider kind: '{}'", other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_kind_round_trip() {
        assert_eq!("openai".parse::<ProviderKind>().unwrap(), ProviderKind::OpenAICompatible);
        assert_eq!("anthropic".parse::<ProviderKind>().unwrap(), ProviderKind::Anthropic);
        assert_eq!("gemini".parse::<ProviderKind>().unwrap(), ProviderKind::Gemini);
        assert!("unknown".parse::<ProviderKind>().is_err());
    }

    #[test]
    fn test_default_endpoints() {
        assert_eq!(ProviderKind::OpenAICompatible.default_endpoint().as_str(), "https://api.openai.com/v1");
        assert_eq!(ProviderKind::Anthropic.default_endpoint().as_str(), "https://api.anthropic.com/v1");
        assert_eq!(ProviderKind::Gemini.default_endpoint().as_str(), "https://generativelanguage.googleapis.com/v1beta");
    }
}
