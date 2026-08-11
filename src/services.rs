use std::{fs, path::PathBuf};

use reqwest::Url;
use serde_json::Value;

use crate::{
    config::Config,
    integrations::{
        ceph::{CephCatalog, CephClient},
        grafana::GrafanaClient,
        kubernetes::{KubernetesCatalog, KubernetesConfig},
        tekton::TektonClient,
    },
};

#[derive(Clone)]
pub struct Services {
    pub(crate) grafana: GrafanaClient,
    pub(crate) tekton: TektonClient,
    pub(crate) kubernetes: KubernetesCatalog,
    pub(crate) ceph: CephCatalog,
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
        for cluster in &config.integrations.ceph.clusters {
            redactions.push(cluster.username.expose().to_owned());
            redactions.push(cluster.password.expose().to_owned());
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
            kubernetes: KubernetesCatalog::new(
                config
                    .integrations
                    .kubernetes
                    .clusters
                    .iter()
                    .map(|cluster| {
                        KubernetesConfig::new(
                            PathBuf::from(&config.integrations.kubernetes.kubectl_path),
                            PathBuf::from(&cluster.kubeconfig),
                            PathBuf::from(&cluster.cache_dir),
                            cluster.context.clone(),
                            cluster.name.clone(),
                        )
                    })
                    .collect(),
            )?,
            ceph: CephCatalog::new(
                config
                    .integrations
                    .ceph
                    .clusters
                    .iter()
                    .map(|cluster| {
                        CephClient::new(
                            cluster.origin.clone(),
                            cluster.username.clone(),
                            cluster.password.clone(),
                        )
                        .map(|client| (cluster.name.clone(), client))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(grafana: GrafanaClient) -> Self {
        Self {
            grafana,
            tekton: TektonClient::disabled_for_test(),
            kubernetes: KubernetesCatalog::inert_for_test(),
            ceph: CephCatalog::disabled_for_test(),
        }
    }
}
