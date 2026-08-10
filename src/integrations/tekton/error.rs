use crate::tool_error::ToolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidArguments,
    CapacityExhausted,
    Timeout,
    Unauthorized,
    NotFound,
    QueryRejected,
    MutationRejected,
    MutationOutcomeUnknown,
    UpstreamUnavailable,
    InvalidResponse,
}

impl Error {
    pub fn into_tool_error(self, subject: &str) -> ToolError {
        let (code, message, retryable) = match self {
            Self::InvalidArguments => ("invalid_arguments", format!("The {subject} arguments are invalid."), false),
            Self::CapacityExhausted => ("capacity_exhausted", "Tekton request capacity is currently exhausted.".to_owned(), true),
            Self::Timeout => ("timeout", "The Tekton query timed out.".to_owned(), true),
            Self::Unauthorized => ("tekton_unauthorized", "An upstream service rejected the MCP service credentials.".to_owned(), false),
            Self::NotFound => ("not_found", format!("The requested {subject} was not found."), false),
            Self::QueryRejected => ("query_rejected", format!("An upstream service rejected the {subject} query."), false),
            Self::MutationRejected => ("mutation_rejected", "The requested Tekton mutation was rejected.".to_owned(), false),
            Self::MutationOutcomeUnknown => ("mutation_outcome_unknown", "The Tekton mutation did not complete cleanly; its outcome may be uncertain. Inspect current runs before retrying.".to_owned(), false),
            Self::UpstreamUnavailable => ("upstream_unavailable", "A Tekton integration dependency is currently unavailable.".to_owned(), true),
            Self::InvalidResponse => ("invalid_response", "A Tekton integration dependency returned an invalid response.".to_owned(), false),
        };
        ToolError::new(code, message, retryable)
    }
}
