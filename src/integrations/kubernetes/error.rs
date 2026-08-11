use std::fmt;

use crate::tool_error::ToolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidArguments,
    CapacityExhausted,
    Timeout,
    Unauthorized,
    NotFound,
    UnsupportedApi,
    QueryRejected,
    MutationRejected,
    MutationOutcomeUnknown,
    UpstreamUnavailable,
    InvalidResponse,
    ResponseTooLarge,
    RequestCancelled,
}

impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalid_arguments",
            Self::CapacityExhausted => "capacity_exhausted",
            Self::Timeout => "timeout",
            Self::Unauthorized => "kubernetes_unauthorized",
            Self::NotFound => "not_found",
            Self::UnsupportedApi => "unsupported_api",
            Self::QueryRejected => "query_rejected",
            Self::MutationRejected => "mutation_rejected",
            Self::MutationOutcomeUnknown => "mutation_outcome_unknown",
            Self::UpstreamUnavailable => "upstream_unavailable",
            Self::InvalidResponse => "invalid_response",
            Self::ResponseTooLarge => "response_too_large",
            Self::RequestCancelled => "request_cancelled",
        }
    }

    pub fn into_tool_error(self, subject: &str) -> ToolError {
        let (message, retryable) = match self {
            Self::InvalidArguments => (format!("The Kubernetes {subject} arguments are invalid."), false),
            Self::CapacityExhausted => ("Kubernetes request capacity is currently exhausted.".to_owned(), true),
            Self::Timeout => ("The Kubernetes query timed out.".to_owned(), true),
            Self::Unauthorized => ("Kubernetes rejected the MCP service credentials.".to_owned(), false),
            Self::NotFound => (format!("The requested Kubernetes {subject} was not found."), false),
            Self::UnsupportedApi => ("The requested Kubernetes API is not supported by this cluster.".to_owned(), false),
            Self::QueryRejected => (format!("Kubernetes rejected the {subject} query."), false),
            Self::MutationRejected => ("The requested Kubernetes mutation was rejected.".to_owned(), false),
            Self::MutationOutcomeUnknown => ("The Kubernetes mutation did not complete cleanly; its outcome may be uncertain. Inspect the exact object before retrying.".to_owned(), false),
            Self::UpstreamUnavailable => ("The Kubernetes API is currently unavailable.".to_owned(), true),
            Self::InvalidResponse => ("Kubernetes returned an invalid response.".to_owned(), false),
            Self::ResponseTooLarge => ("The Kubernetes response exceeded the safe size limit.".to_owned(), false),
            Self::RequestCancelled => ("The Kubernetes request was cancelled.".to_owned(), false),
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
    fn tool_errors_are_structured_stable_and_secret_free() {
        for (error, code, retryable) in [
            (Error::CapacityExhausted, "capacity_exhausted", true),
            (Error::Unauthorized, "kubernetes_unauthorized", false),
            (Error::MutationRejected, "mutation_rejected", false),
            (
                Error::MutationOutcomeUnknown,
                "mutation_outcome_unknown",
                false,
            ),
            (Error::InvalidResponse, "invalid_response", false),
        ] {
            let result = error.into_tool_error("resource").into_mcp_result().raw;
            assert_eq!(result["isError"], true);
            assert_eq!(result["structuredContent"]["error"]["code"], code);
            assert_eq!(result["structuredContent"]["error"]["retryable"], retryable);
            assert!(!result.to_string().contains("kubeconfig"));
            assert!(!result.to_string().contains("bearer"));
            assert!(!result.to_string().contains("stderr"));
        }
    }
}
