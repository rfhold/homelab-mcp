use std::{collections::BTreeMap, future::Future, sync::atomic::Ordering, time::Duration};

use serde_json::{Value, json};

use super::{
    CephClient, Error,
    actions::{ExecCommand, QueryCommand, valid_cluster},
    client::new_dispatch_progress,
};

const MAX_CLUSTERS: usize = 32;

#[derive(Clone)]
pub struct CephCatalog {
    clients: BTreeMap<String, CephClient>,
}

impl CephCatalog {
    pub fn new(entries: impl IntoIterator<Item = (String, CephClient)>) -> Result<Self, String> {
        let mut clients = BTreeMap::new();
        for (name, client) in entries {
            if !valid_cluster(&name) {
                return Err("invalid Ceph cluster name".into());
            }
            if clients.len() >= MAX_CLUSTERS {
                return Err("too many Ceph clusters".into());
            }
            if clients.insert(name, client).is_some() {
                return Err("duplicate Ceph cluster name".into());
            }
        }
        Ok(Self { clients })
    }

    #[cfg(test)]
    pub(crate) fn disabled_for_test() -> Self {
        Self::new([("test-cluster".into(), CephClient::disabled_for_test())]).unwrap()
    }

    pub async fn query(&self, command: &QueryCommand) -> Result<Value, Error> {
        let Some(cluster) = command.cluster() else {
            let result = self
                .clients
                .keys()
                .map(|cluster| json!({"cluster":cluster}))
                .collect::<Vec<_>>();
            return Ok(json!({"result":result,"truncated":false}));
        };
        let client = self.client(cluster)?;
        let operation = async {
            match command {
                QueryCommand::ClusterList => unreachable!(),
                QueryCommand::StatusGet { .. } => client.status(cluster).await,
                QueryCommand::MetricsSummary { .. } => client.metrics_summary(cluster).await,
                QueryCommand::OsdList { limit, .. } => client.osds(cluster, *limit).await,
                QueryCommand::OsdGet { osd_id, .. } => client.osd(cluster, *osd_id).await,
                QueryCommand::OsdSafeToDestroy { osd_id, .. } => {
                    client.safe_to_destroy(cluster, *osd_id).await
                }
                QueryCommand::DeviceList { osd_id, limit, .. } => {
                    client.devices(cluster, *osd_id, *limit).await
                }
                QueryCommand::DeviceGet {
                    osd_id, device_id, ..
                } => client.device(cluster, *osd_id, device_id).await,
                QueryCommand::FlagsGet { .. } => client.flags(cluster).await,
                QueryCommand::TaskList { limit, .. } => client.tasks(cluster, *limit).await,
            }
        };
        with_deadline(client.operation_timeout(), Error::Timeout, operation).await
    }

    pub async fn execute(&self, command: &ExecCommand) -> Result<Value, Error> {
        let cluster = command.cluster();
        let client = self.client(cluster)?;
        let progress = new_dispatch_progress();
        let operation = async {
            match command {
                ExecCommand::Mark { osd_id, state, .. } => {
                    client
                        .mark_with_progress(cluster, *osd_id, *state, &progress)
                        .await
                }
                ExecCommand::Reweight { osd_id, weight, .. } => {
                    client
                        .reweight_with_progress(cluster, *osd_id, *weight, &progress)
                        .await
                }
                ExecCommand::Scrub { osd_id, kind, .. } => {
                    client
                        .scrub_with_progress(cluster, *osd_id, *kind, &progress)
                        .await
                }
                ExecCommand::Destroy { osd_id, .. } => {
                    client
                        .destroy_with_progress(cluster, *osd_id, &progress)
                        .await
                }
                ExecCommand::Purge { osd_id, .. } => {
                    client
                        .purge_with_progress(cluster, *osd_id, &progress)
                        .await
                }
            }
        };
        tokio::time::timeout(client.operation_timeout(), operation)
            .await
            .map_err(|_| {
                if progress.load(Ordering::SeqCst) {
                    Error::MutationOutcomeUnknown
                } else {
                    Error::Timeout
                }
            })?
    }

    fn client(&self, cluster: &str) -> Result<&CephClient, Error> {
        self.clients.get(cluster).ok_or(Error::InvalidArguments)
    }
}

async fn with_deadline<T>(
    duration: Duration,
    timeout_error: Error,
    operation: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::time::timeout(duration, operation)
        .await
        .map_err(|_| timeout_error)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dynamic_catalog_is_sorted_and_enforces_membership() {
        let catalog = CephCatalog::new([
            ("zeus".into(), CephClient::disabled_for_test()),
            ("apollo-2".into(), CephClient::disabled_for_test()),
        ])
        .unwrap();
        let result = catalog.query(&QueryCommand::ClusterList).await.unwrap();
        assert_eq!(
            result["result"],
            json!([{"cluster":"apollo-2"},{"cluster":"zeus"}])
        );
        assert_eq!(
            catalog
                .query(&QueryCommand::StatusGet {
                    cluster: "missing".into()
                })
                .await
                .unwrap_err(),
            Error::InvalidArguments
        );
    }

    #[tokio::test]
    async fn catalog_accepts_empty_and_rejects_duplicate_invalid_and_oversized_configuration() {
        let empty = CephCatalog::new(std::iter::empty::<(String, CephClient)>()).unwrap();
        assert_eq!(
            empty.query(&QueryCommand::ClusterList).await.unwrap()["result"],
            json!([])
        );
        assert!(
            CephCatalog::new([
                ("same".into(), CephClient::disabled_for_test()),
                ("same".into(), CephClient::disabled_for_test()),
            ])
            .is_err()
        );
        assert!(CephCatalog::new([("Bad.Name".into(), CephClient::disabled_for_test())]).is_err());
        let entries = (0..=MAX_CLUSTERS)
            .map(|index| (format!("cluster-{index}"), CephClient::disabled_for_test()));
        assert!(CephCatalog::new(entries).is_err());
        let catalog = CephCatalog::disabled_for_test();
        assert!(catalog.query(&QueryCommand::ClusterList).await.is_ok());
    }

    #[tokio::test]
    async fn deadline_caps_the_sum_of_individually_short_phases() {
        let query = async {
            tokio::time::sleep(Duration::from_millis(15)).await;
            tokio::time::sleep(Duration::from_millis(15)).await;
            Ok::<_, Error>(())
        };
        assert_eq!(
            with_deadline(Duration::from_millis(20), Error::Timeout, query).await,
            Err(Error::Timeout)
        );

        let mutation = async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            tokio::time::sleep(Duration::from_millis(10)).await;
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, Error>(())
        };
        assert_eq!(
            with_deadline(
                Duration::from_millis(25),
                Error::MutationOutcomeUnknown,
                mutation
            )
            .await,
            Err(Error::MutationOutcomeUnknown)
        );
    }
}
