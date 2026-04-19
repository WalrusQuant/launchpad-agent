use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("model provider error: {0}")]
    Provider(#[from] anyhow::Error),

    #[error("max turns ({0}) exceeded")]
    MaxTurnsExceeded(usize),

    #[error("context too long after compaction")]
    ContextTooLong,

    #[error("session aborted by user")]
    Aborted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages() {
        let err = AgentError::MaxTurnsExceeded(10);
        assert_eq!(err.to_string(), "max turns (10) exceeded");

        let err = AgentError::ContextTooLong;
        assert_eq!(err.to_string(), "context too long after compaction");

        let err = AgentError::Aborted;
        assert_eq!(err.to_string(), "session aborted by user");
    }

    #[test]
    fn provider_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("connection refused");
        let err: AgentError = anyhow_err.into();
        assert!(err.to_string().contains("connection refused"));
    }
}
