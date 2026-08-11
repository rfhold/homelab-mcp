use std::fmt;

use crate::tool_error::ToolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidArguments,
    CapacityExhausted,
    Timeout,
    Unauthorized,
    NotFound,
    UnsupportedAction,
    QueryRejected,
    MutationRejected,
    MutationOutcomeUnknown,
    UnsafeToDestroy,
    UpstreamUnavailable,
    InvalidResponse,
    ResponseTooLarge,
}

impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalid_arguments",
            Self::CapacityExhausted => "capacity_exhausted",
            Self::Timeout => "timeout",
            Self::Unauthorized => "ceph_unauthorized",
            Self::NotFound => "not_found",
            Self::UnsupportedAction => "unsupported_action",
            Self::QueryRejected => "query_rejected",
            Self::MutationRejected => "mutation_rejected",
            Self::MutationOutcomeUnknown => "mutation_outcome_unknown",
            Self::UnsafeToDestroy => "unsafe_to_destroy",
            Self::UpstreamUnavailable => "upstream_unavailable",
            Self::InvalidResponse => "invalid_response",
            Self::ResponseTooLarge => "response_too_large",
        }
    }

    pub fn into_tool_error(self, subject: &str) -> ToolError {
        let (message, retryable) = match self {
            Self::InvalidArguments => (format!("The Ceph {subject} arguments are invalid."), false),
            Self::CapacityExhausted => ("Ceph request capacity is currently exhausted.".into(), true),
            Self::Timeout => ("The Ceph query timed out.".into(), true),
            Self::Unauthorized => ("Ceph Dashboard rejected the service credentials.".into(), false),
            Self::NotFound => (format!("The requested Ceph {subject} was not found."), false),
            Self::UnsupportedAction => ("The requested action has no verified Ceph Squid Dashboard endpoint.".into(), false),
            Self::QueryRejected => (format!("Ceph Dashboard rejected the {subject} query."), false),
            Self::MutationRejected => ("Ceph Dashboard rejected the requested mutation.".into(), false),
            Self::MutationOutcomeUnknown => ("The Ceph mutation did not complete cleanly; its outcome may be uncertain. Inspect the cluster before retrying.".into(), false),
            Self::UnsafeToDestroy => ("Ceph reports that the OSD is not safe to destroy.".into(), false),
            Self::UpstreamUnavailable => ("Ceph Dashboard is currently unavailable.".into(), true),
            Self::InvalidResponse => ("Ceph Dashboard returned an invalid response.".into(), false),
            Self::ResponseTooLarge => ("The Ceph Dashboard response exceeded the safe size limit.".into(), false),
        };
        ToolError::new(self.code(), message, retryable)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_stable_nonretryable_and_secret_free() {
        for (error, code, retryable) in [
            (Error::Unauthorized, "ceph_unauthorized", false),
            (Error::MutationRejected, "mutation_rejected", false),
            (
                Error::MutationOutcomeUnknown,
                "mutation_outcome_unknown",
                false,
            ),
            (Error::UnsafeToDestroy, "unsafe_to_destroy", false),
            (Error::UpstreamUnavailable, "upstream_unavailable", true),
            (Error::Timeout, "timeout", true),
            (Error::InvalidResponse, "invalid_response", false),
        ] {
            let result = error.into_tool_error("osd").into_mcp_result().raw;
            assert_eq!(result["structuredContent"]["error"]["code"], code);
            assert_eq!(result["structuredContent"]["error"]["retryable"], retryable);
            let text = result.to_string();
            assert!(!text.contains("dashboard-password"));
            assert!(!text.contains("eyJ"));
            assert!(!text.contains("response body"));
        }
    }
}
