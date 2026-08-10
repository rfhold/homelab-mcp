use crate::{config::IntegrationsConfig, integrations::grafana::GrafanaClient};

#[derive(Clone)]
pub struct Services {
    pub(crate) grafana: GrafanaClient,
}

impl Services {
    pub fn production(config: &IntegrationsConfig) -> Result<Self, String> {
        Ok(Self {
            grafana: GrafanaClient::production(
                config.grafana.origin.clone(),
                config.grafana.token.clone(),
            )?,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(grafana: GrafanaClient) -> Self {
        Self { grafana }
    }
}
