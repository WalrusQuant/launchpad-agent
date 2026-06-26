//! Secret detection and redaction: scans model-visible text for credentials
//! and replaces each with a fixed placeholder. Split out of `lib.rs`, which
//! retains the permission and sandbox policy types. Re-exported from the crate
//! root via `pub use redaction::*` so the public API is unchanged.

use std::collections::HashSet;
use std::sync::Arc;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// The fixed placeholder inserted when a secret is redacted from model-visible text.
pub const REDACTED_SECRET_PLACEHOLDER: &str = "[REDACTED_SECRET]";

/// Describes the confidence level of one secret detection match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SecretMatchConfidence {
    /// The detector is weakly confident the match is a real secret.
    Low,
    /// The detector is moderately confident the match is a real secret.
    Medium,
    /// The detector is strongly confident the match is a real secret.
    High,
}

/// Describes one secret substring identified by a detector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretMatch {
    /// The byte offset where the secret starts.
    pub start: usize,
    /// The byte offset immediately after the secret ends.
    pub end: usize,
    /// The placeholder that should replace the secret.
    pub placeholder: String,
    /// The detector confidence for this match.
    pub confidence: SecretMatchConfidence,
}

/// Provides deterministic secret detection over model-bound text.
pub trait SecretDetector: Send + Sync {
    /// Returns the stable identifier for the detector implementation.
    fn detector_id(&self) -> &'static str;

    /// Returns every secret match detected in the supplied input.
    fn detect(&self, input: &str) -> Vec<SecretMatch>;
}

/// Exposes the active set of secret detectors.
pub trait SecretDetectorRegistry: Send + Sync {
    /// Returns every configured detector.
    fn all(&self) -> Vec<Arc<dyn SecretDetector>>;
}

/// Stores one accepted secret match together with the detector that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedSecretMatch {
    /// The stable detector identifier.
    pub detector_id: String,
    /// The accepted secret match.
    pub matched: SecretMatch,
}

/// Stores the telemetry for one redaction run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RedactionReport {
    /// The accepted matches, in application order.
    pub matches: Vec<AcceptedSecretMatch>,
}

/// Stores the result of applying deterministic redaction to one text fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionResult {
    /// The redacted text safe for model visibility.
    pub redacted_text: String,
    /// The redaction telemetry emitted during processing.
    pub report: RedactionReport,
}

/// Regex-based secret detector used by the default detector set.
pub struct RegexSecretDetector {
    /// The stable detector identifier.
    pub detector_id_value: &'static str,
    /// The compiled regex used for detection.
    pub regex: Regex,
    /// The placeholder used to replace matching text.
    pub placeholder: &'static str,
    /// The confidence assigned to each match.
    pub confidence: SecretMatchConfidence,
}

impl SecretDetector for RegexSecretDetector {
    fn detector_id(&self) -> &'static str {
        self.detector_id_value
    }

    fn detect(&self, input: &str) -> Vec<SecretMatch> {
        self.regex
            .find_iter(input)
            .map(|matched| SecretMatch {
                start: matched.start(),
                end: matched.end(),
                placeholder: self.placeholder.to_string(),
                confidence: self.confidence,
            })
            .collect()
    }
}

/// In-memory detector registry used by the runtime and tests.
#[derive(Default)]
pub struct InMemorySecretDetectorRegistry {
    /// The detectors owned by the registry.
    pub detectors: Vec<Arc<dyn SecretDetector>>,
}

impl InMemorySecretDetectorRegistry {
    /// Creates the default registry with the required built-in regex detectors.
    pub fn with_default_detectors() -> Self {
        let detectors: Vec<Arc<dyn SecretDetector>> = vec![
            Arc::new(RegexSecretDetector {
                detector_id_value: "openai_api_key",
                regex: Regex::new(r"sk-[A-Za-z0-9]{20,}").expect("valid regex"),
                placeholder: REDACTED_SECRET_PLACEHOLDER,
                confidence: SecretMatchConfidence::High,
            }),
            Arc::new(RegexSecretDetector {
                detector_id_value: "aws_access_key_id",
                regex: Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("valid regex"),
                placeholder: REDACTED_SECRET_PLACEHOLDER,
                confidence: SecretMatchConfidence::High,
            }),
            Arc::new(RegexSecretDetector {
                detector_id_value: "bearer_token",
                regex: Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._\-]{16,}\b").expect("valid regex"),
                placeholder: REDACTED_SECRET_PLACEHOLDER,
                confidence: SecretMatchConfidence::High,
            }),
            Arc::new(RegexSecretDetector {
                detector_id_value: "password_assignment",
                regex: Regex::new(
                    r#"(?i)\b(api[_-]?key|token|secret|password)\b(\s*[:=]\s*)(["']?)[^\s"']{8,}"#,
                )
                .expect("valid regex"),
                placeholder: REDACTED_SECRET_PLACEHOLDER,
                confidence: SecretMatchConfidence::Medium,
            }),
        ];

        Self { detectors }
    }
}

impl SecretDetectorRegistry for InMemorySecretDetectorRegistry {
    fn all(&self) -> Vec<Arc<dyn SecretDetector>> {
        self.detectors.clone()
    }
}

/// Applies deterministic secret redaction using a detector registry.
pub struct SecretRedactor {
    /// The detector registry used during redaction.
    pub registry: Arc<dyn SecretDetectorRegistry>,
}

impl SecretRedactor {
    /// Creates a new secret redactor.
    pub fn new(registry: Arc<dyn SecretDetectorRegistry>) -> Self {
        Self { registry }
    }

    /// Redacts every accepted secret match from one input fragment.
    pub fn redact(&self, input: &str) -> RedactionResult {
        let accepted = self.merge_matches(input);
        let mut redacted = String::with_capacity(input.len());
        let mut cursor = 0usize;

        for accepted_match in &accepted {
            redacted.push_str(&input[cursor..accepted_match.matched.start]);
            redacted.push_str(&accepted_match.matched.placeholder);
            cursor = accepted_match.matched.end;
        }
        redacted.push_str(&input[cursor..]);

        RedactionResult {
            redacted_text: redacted,
            report: RedactionReport { matches: accepted },
        }
    }

    fn merge_matches(&self, input: &str) -> Vec<AcceptedSecretMatch> {
        let mut all_matches = self
            .registry
            .all()
            .into_iter()
            .flat_map(|detector| {
                let detector_id = detector.detector_id().to_string();
                detector
                    .detect(input)
                    .into_iter()
                    .map(move |matched| AcceptedSecretMatch {
                        detector_id: detector_id.clone(),
                        matched,
                    })
            })
            .collect::<Vec<_>>();

        all_matches.sort_by(|left, right| {
            let left_len = left.matched.end.saturating_sub(left.matched.start);
            let right_len = right.matched.end.saturating_sub(right.matched.start);
            right_len
                .cmp(&left_len)
                .then(right.matched.confidence.cmp(&left.matched.confidence))
                .then(left.matched.start.cmp(&right.matched.start))
                .then(left.detector_id.cmp(&right.detector_id))
        });

        let mut occupied = HashSet::new();
        let mut accepted = Vec::new();

        for candidate in all_matches {
            if (candidate.matched.start..candidate.matched.end)
                .any(|index| occupied.contains(&index))
            {
                continue;
            }

            for index in candidate.matched.start..candidate.matched.end {
                occupied.insert(index);
            }
            accepted.push(candidate);
        }

        accepted.sort_by_key(|entry| entry.matched.start);
        accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_redactor_detects_and_redacts_openai_keys() {
        let registry = InMemorySecretDetectorRegistry::with_default_detectors();
        let redactor = SecretRedactor::new(std::sync::Arc::new(registry));
        let result = redactor.redact("token sk-123456789012345678901234");

        assert!(result.redacted_text.contains(REDACTED_SECRET_PLACEHOLDER));
        assert_eq!(result.report.matches.len(), 1);
        assert_eq!(
            result.report.matches[0].matched.confidence,
            SecretMatchConfidence::High
        );
    }

    #[test]
    fn overlapping_matches_choose_longest_then_highest_confidence() {
        struct TestRegistry {
            detectors: Vec<std::sync::Arc<dyn super::SecretDetector>>,
        }

        impl SecretDetectorRegistry for TestRegistry {
            fn all(&self) -> Vec<std::sync::Arc<dyn super::SecretDetector>> {
                self.detectors.clone()
            }
        }

        let registry = TestRegistry {
            detectors: vec![
                std::sync::Arc::new(RegexSecretDetector {
                    detector_id_value: "short",
                    regex: Regex::new("abcdef").expect("regex"),
                    placeholder: REDACTED_SECRET_PLACEHOLDER,
                    confidence: SecretMatchConfidence::Low,
                }),
                std::sync::Arc::new(RegexSecretDetector {
                    detector_id_value: "long",
                    regex: Regex::new("abcdefgh").expect("regex"),
                    placeholder: REDACTED_SECRET_PLACEHOLDER,
                    confidence: SecretMatchConfidence::Medium,
                }),
            ],
        };

        let redactor = SecretRedactor::new(std::sync::Arc::new(registry));
        let result = redactor.redact("zzabcdefghyy");

        assert_eq!(result.report.matches.len(), 1);
        assert_eq!(result.report.matches[0].detector_id, "long");
    }
}
