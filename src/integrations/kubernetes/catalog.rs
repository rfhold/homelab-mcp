use std::{collections::BTreeMap, future::Future};

use super::{
    Error, KubernetesClient, KubernetesConfig,
    actions::{ExecCommand, QueryCommand},
    normalize::{ClusterCatalog, ExecResult, ListMetadata, QueryResult},
};

const MAX_CLUSTERS: usize = 32;

#[derive(Clone)]
pub struct KubernetesCatalog {
    clients: BTreeMap<String, KubernetesClient>,
}

impl KubernetesCatalog {
    pub fn new(configs: Vec<KubernetesConfig>) -> Result<Self, String> {
        if configs.is_empty() || configs.len() > MAX_CLUSTERS {
            return Err("invalid Kubernetes cluster catalog".into());
        }
        let mut clients = BTreeMap::new();
        for config in configs {
            let name = config.cluster_name.clone();
            let client = KubernetesClient::new(config)?;
            if clients.insert(name, client).is_some() {
                return Err("duplicate Kubernetes cluster name".into());
            }
        }
        Ok(Self { clients })
    }

    pub fn clusters(&self) -> QueryResult {
        let clusters = self
            .clients
            .values()
            .map(KubernetesClient::cluster)
            .collect::<Vec<_>>();
        let count = clusters.len() as u16;
        QueryResult::Clusters(ClusterCatalog {
            clusters,
            metadata: ListMetadata {
                pages: 0,
                inspected: count,
                returned: count,
                source_truncated: false,
                result_truncated: false,
                aggregate_truncated: false,
                truncated: false,
            },
        })
    }

    pub async fn dispatch_cancelled(
        &self,
        command: &QueryCommand,
        cancellation: impl Future<Output = ()> + Send,
    ) -> Result<QueryResult, Error> {
        let mut cancellation = Box::pin(cancellation);
        match command {
            QueryCommand::ClusterList => Ok(self.clusters()),
            QueryCommand::CapabilityList { cluster, query } => {
                self.client(cluster)?
                    .capabilities_cancelled(query, cancellation.as_mut())
                    .await
            }
            QueryCommand::Resource { cluster, query } => {
                self.client(cluster)?
                    .query_cancelled(query, cancellation.as_mut())
                    .await
            }
        }
    }

    pub async fn execute_cancelled(
        &self,
        command: &ExecCommand,
        cancellation: impl Future<Output = ()> + Send,
    ) -> Result<ExecResult, Error> {
        let mut cancellation = Box::pin(cancellation);
        self.clients
            .get(command.cluster())
            .ok_or(Error::MutationRejected)?
            .execute_cancelled(command, cancellation.as_mut())
            .await
    }

    fn client(&self, cluster: &str) -> Result<&KubernetesClient, Error> {
        self.clients.get(cluster).ok_or(Error::NotFound)
    }

    #[cfg(test)]
    pub(crate) fn inert_for_test() -> Self {
        Self::new(vec![KubernetesConfig::new(
            "/inert/kubectl".into(),
            "/inert/kubeconfig".into(),
            "/inert/cache".into(),
            "test-context".to_owned(),
            "test".to_owned(),
        )])
        .expect("inert Kubernetes test catalog must be valid")
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use super::*;

    fn config(name: &str) -> KubernetesConfig {
        let mut config = KubernetesConfig::new(
            PathBuf::from("/usr/bin/kubectl"),
            PathBuf::from("/tmp/kubeconfig"),
            PathBuf::from("/tmp/kubectl-cache"),
            format!("context-{name}"),
            name.to_owned(),
        );
        config.deadline = Duration::from_secs(1);
        config
    }

    #[test]
    fn catalog_is_bounded_unique_and_stably_sorted() {
        assert!(KubernetesCatalog::new(Vec::new()).is_err());
        assert!(KubernetesCatalog::new(vec![config("same"), config("same")]).is_err());
        let catalog = KubernetesCatalog::new(vec![config("zeta"), config("alpha")]).unwrap();
        let QueryResult::Clusters(result) = catalog.clusters() else {
            panic!()
        };
        assert_eq!(
            result
                .clusters
                .iter()
                .map(|cluster| cluster.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(result.metadata.returned, 2);
        assert!(!result.metadata.truncated);
    }
}
