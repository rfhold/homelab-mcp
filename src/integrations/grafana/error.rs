use crate::tool_error::ToolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidArguments,
    CapacityExhausted,
    Timeout,
    Unauthorized,
    QueryRejected,
    MutationRejected,
    MutationOutcomeUnknown,
    UpstreamUnavailable,
    InvalidResponse,
    RenderTimeout,
    RenderRejected,
    RenderInvalidResponse,
}

impl Error {
    pub fn into_tool_error(self, query_name: &str) -> ToolError {
        let invalid_arguments = format!("The {query_name} arguments are invalid.");
        let query_rejected = format!("Grafana rejected the {query_name} query.");
        let (code, message, retryable) = match self {
            Self::InvalidArguments => ("invalid_arguments", invalid_arguments, false),
            Self::CapacityExhausted => (
                "capacity_exhausted",
                "Grafana request capacity is currently exhausted.".to_owned(),
                true,
            ),
            Self::Timeout => ("timeout", "The Grafana query timed out.".to_owned(), true),
            Self::Unauthorized => (
                "grafana_unauthorized",
                "Grafana rejected the service credentials.".to_owned(),
                false,
            ),
            Self::QueryRejected => ("query_rejected", query_rejected, false),
            Self::MutationRejected => (
                "mutation_rejected",
                "Grafana rejected the requested mutation.".to_owned(),
                false,
            ),
            Self::MutationOutcomeUnknown => (
                "mutation_outcome_unknown",
                "The Grafana mutation did not complete cleanly; its outcome may be uncertain. Check existing silences before retrying.".to_owned(),
                false,
            ),
            Self::UpstreamUnavailable => (
                "upstream_unavailable",
                "Grafana is currently unavailable.".to_owned(),
                true,
            ),
            Self::InvalidResponse => (
                "invalid_response",
                "Grafana returned an invalid response.".to_owned(),
                false,
            ),
            Self::RenderTimeout => (
                "render_timeout",
                "The Grafana render timed out.".to_owned(),
                true,
            ),
            Self::RenderRejected => (
                "render_rejected",
                "Grafana rejected the render request.".to_owned(),
                false,
            ),
            Self::RenderInvalidResponse => (
                "render_invalid_response",
                "Grafana returned an invalid render response.".to_owned(),
                false,
            ),
        };
        ToolError::new(code, message, retryable)
    }
}
