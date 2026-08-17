use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeployError {
    InvalidConfiguration,
    InvalidCatalog,
    DeployNotFound,
    DeployUnavailable,
    MissingHostPin,
    Busy,
    CredentialUnavailable,
    CredentialInvalid,
    ExecutionRejected,
    Cancelled,
    ExecutionOutcomeUnknown,
    TimeoutOutcomeUnknown,
    CancelledOutcomeUnknown,
    OutputTooLargeOutcomeUnknown,
    InvalidOutput,
}

impl DeployError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidConfiguration => "invalid_configuration",
            Self::InvalidCatalog => "invalid_catalog",
            Self::DeployNotFound => "deploy_not_found",
            Self::DeployUnavailable => "deploy_unavailable",
            Self::MissingHostPin => "missing_host_pin",
            Self::Busy => "deploy_busy",
            Self::CredentialUnavailable => "credential_unavailable",
            Self::CredentialInvalid => "credential_invalid",
            Self::ExecutionRejected => "execution_rejected",
            Self::Cancelled => "cancelled",
            Self::ExecutionOutcomeUnknown => "execution_outcome_unknown",
            Self::TimeoutOutcomeUnknown => "timeout_outcome_unknown",
            Self::CancelledOutcomeUnknown => "cancelled_outcome_unknown",
            Self::OutputTooLargeOutcomeUnknown => "output_too_large_outcome_unknown",
            Self::InvalidOutput => "invalid_output",
        }
    }
}

impl fmt::Display for DeployError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for DeployError {}
