use std::fs;

use reqwest::Url;
use serde_json::Value;

use crate::{
    config::Config,
    integrations::{grafana::GrafanaClient, tekton::TektonClient},
};

#[derive(Clone)]
pub struct Services {
    pub(crate) grafana: GrafanaClient,
    pub(crate) tekton: TektonClient,
}

impl Services {
    pub fn production(config: &Config) -> Result<Self, String> {
        let mut redactions = vec![
            config.database.url.clone(),
            config.oidc.client_secret.expose().to_owned(),
            config.integrations.grafana.token.expose().to_owned(),
        ];
        if let Ok(database_url) = Url::parse(&config.database.url)
            && let Some(password) = database_url.password()
        {
            redactions.push(password.to_owned());
        }
        let keyring = fs::read(&config.oauth.wrapping_keys_file)
            .map_err(|_| "failed to read OAuth wrapping keyring for log redaction".to_owned())?;
        let keyring: Value = serde_json::from_slice(&keyring)
            .map_err(|_| "invalid OAuth wrapping keyring for log redaction".to_owned())?;
        if let Some(keys) = keyring["keys"].as_array() {
            redactions.extend(
                keys.iter()
                    .filter_map(|entry| entry["key"].as_str())
                    .map(str::to_owned),
            );
        }
        Ok(Self {
            grafana: GrafanaClient::production(
                config.integrations.grafana.origin.clone(),
                config.integrations.grafana.token.clone(),
            )?,
            tekton: TektonClient::production(&config.integrations.tekton, redactions)?,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(grafana: GrafanaClient) -> Self {
        Self {
            grafana,
            tekton: TektonClient::disabled_for_test(),
        }
    }
}
