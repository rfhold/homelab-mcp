use std::{fs, future::Future, path::PathBuf, pin::Pin, sync::Arc};

use reqwest::Url;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    config::Config,
    integrations::deploys::{
        DeployCatalog, DeployError, DeployMetadata, DeployResult, DeployRunner, DeployRunnerConfig,
        OpenBaoClient, OpenBaoConfig,
    },
    integrations::{
        ceph::{CephCatalog, CephClient},
        grafana::GrafanaClient,
        kubernetes::{KubernetesCatalog, KubernetesConfig},
        tekton::TektonClient,
    },
    inventory::{CreateMachine, Machine, MachineRepository, RepositoryError, UpdateMachine},
};

pub(crate) type ServiceFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait InventoryService: Send + Sync {
    fn create(&self, input: CreateMachine) -> ServiceFuture<'_, Result<Machine, RepositoryError>>;
    fn get(&self, id: Uuid) -> ServiceFuture<'_, Result<Machine, RepositoryError>>;
    fn list(&self, limit: u16) -> ServiceFuture<'_, Result<Vec<Machine>, RepositoryError>>;
    fn update(
        &self,
        id: Uuid,
        input: UpdateMachine,
    ) -> ServiceFuture<'_, Result<Machine, RepositoryError>>;
    fn replace_host_key(
        &self,
        id: Uuid,
        key: String,
    ) -> ServiceFuture<'_, Result<Machine, RepositoryError>>;
    fn clear_host_key(&self, id: Uuid) -> ServiceFuture<'_, Result<Machine, RepositoryError>>;
    fn delete(&self, id: Uuid) -> ServiceFuture<'_, Result<(), RepositoryError>>;
}

impl InventoryService for MachineRepository {
    fn create(&self, input: CreateMachine) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(self.create(input))
    }
    fn get(&self, id: Uuid) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(self.get(id))
    }
    fn list(&self, limit: u16) -> ServiceFuture<'_, Result<Vec<Machine>, RepositoryError>> {
        Box::pin(self.list(limit))
    }
    fn update(
        &self,
        id: Uuid,
        input: UpdateMachine,
    ) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(self.update(id, input))
    }
    fn replace_host_key(
        &self,
        id: Uuid,
        key: String,
    ) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(self.replace_host_key(id, key))
    }
    fn clear_host_key(&self, id: Uuid) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(self.clear_host_key(id))
    }
    fn delete(&self, id: Uuid) -> ServiceFuture<'_, Result<(), RepositoryError>> {
        Box::pin(self.delete(id))
    }
}

pub(crate) trait DeployService: Send + Sync {
    fn list(&self) -> Vec<DeployMetadata>;
    fn run_cancelled<'a>(
        &'a self,
        deploy_id: &'a str,
        machine: Machine,
        cancellation: Pin<&'a mut (dyn Future<Output = ()> + Send)>,
    ) -> ServiceFuture<'a, Result<DeployResult, DeployError>>;
}

impl DeployService for DeployRunner<OpenBaoClient> {
    fn list(&self) -> Vec<DeployMetadata> {
        self.list()
    }

    fn run_cancelled<'a>(
        &'a self,
        deploy_id: &'a str,
        machine: Machine,
        cancellation: Pin<&'a mut (dyn Future<Output = ()> + Send)>,
    ) -> ServiceFuture<'a, Result<DeployResult, DeployError>> {
        Box::pin(self.run_cancelled(deploy_id, machine, cancellation))
    }
}

#[derive(Clone)]
pub struct Services {
    pub(crate) grafana: GrafanaClient,
    pub(crate) tekton: TektonClient,
    pub(crate) kubernetes: KubernetesCatalog,
    pub(crate) ceph: CephCatalog,
    pub(crate) inventory: Arc<dyn InventoryService>,
    pub(crate) deploys: Arc<dyn DeployService>,
}

impl Services {
    pub fn production(config: &Config, pool: PgPool) -> Result<Self, String> {
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
        let deploy_config = &config.integrations.deploys;
        let deploy_catalog =
            DeployCatalog::load(deploy_config.root.clone(), deploy_config.catalog.clone())
                .map_err(|_| "failed to initialize deploy catalog".to_owned())?;
        let signer = OpenBaoClient::new(OpenBaoConfig {
            origin: deploy_config.openbao_origin.clone(),
            kubernetes_auth_mount: deploy_config.openbao_kubernetes_auth_mount.clone(),
            kubernetes_role: deploy_config.openbao_kubernetes_role.clone(),
            ssh_mount: deploy_config.openbao_ssh_mount.clone(),
            ssh_role: deploy_config.openbao_ssh_role.clone(),
            jwt_path: deploy_config.openbao_jwt_path.clone(),
            request_timeout: deploy_config.openbao_request_timeout,
        })
        .map_err(|_| "failed to initialize OpenBao deploy signer".to_owned())?;
        let deploys = DeployRunner::new(
            deploy_catalog,
            signer,
            DeployRunnerConfig {
                uv_executable: deploy_config.uv_executable.clone(),
                ssh_keygen_executable: deploy_config.ssh_keygen_executable.clone(),
                temp_root: deploy_config.temp_root.clone(),
            },
        )
        .map_err(|_| "failed to initialize deploy runner".to_owned())?;
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
            inventory: Arc::new(MachineRepository::new(pool)),
            deploys: Arc::new(deploys),
        })
    }

    #[cfg(test)]
    pub(crate) fn new(grafana: GrafanaClient) -> Self {
        Self {
            grafana,
            tekton: TektonClient::disabled_for_test(),
            kubernetes: KubernetesCatalog::inert_for_test(),
            ceph: CephCatalog::disabled_for_test(),
            inventory: Arc::new(InertInventory),
            deploys: Arc::new(InertDeploys),
        }
    }
}

#[cfg(test)]
pub(crate) struct InertInventory;

#[cfg(test)]
impl InventoryService for InertInventory {
    fn create(&self, _: CreateMachine) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(async { Err(RepositoryError::Database) })
    }
    fn get(&self, _: Uuid) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(async { Err(RepositoryError::NotFound) })
    }
    fn list(&self, _: u16) -> ServiceFuture<'_, Result<Vec<Machine>, RepositoryError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn update(
        &self,
        _: Uuid,
        _: UpdateMachine,
    ) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(async { Err(RepositoryError::NotFound) })
    }
    fn replace_host_key(
        &self,
        _: Uuid,
        _: String,
    ) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(async { Err(RepositoryError::NotFound) })
    }
    fn clear_host_key(&self, _: Uuid) -> ServiceFuture<'_, Result<Machine, RepositoryError>> {
        Box::pin(async { Err(RepositoryError::NotFound) })
    }
    fn delete(&self, _: Uuid) -> ServiceFuture<'_, Result<(), RepositoryError>> {
        Box::pin(async { Err(RepositoryError::NotFound) })
    }
}

#[cfg(test)]
pub(crate) struct InertDeploys;

#[cfg(test)]
impl DeployService for InertDeploys {
    fn list(&self) -> Vec<DeployMetadata> {
        Vec::new()
    }
    fn run_cancelled<'a>(
        &'a self,
        _: &'a str,
        _: Machine,
        _: Pin<&'a mut (dyn Future<Output = ()> + Send)>,
    ) -> ServiceFuture<'a, Result<DeployResult, DeployError>> {
        Box::pin(async { Err(DeployError::DeployNotFound) })
    }
}
