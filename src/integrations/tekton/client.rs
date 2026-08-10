use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use reqwest::{Certificate, Client, Method, StatusCode, Url, redirect::Policy};
use serde::Deserialize as _;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tokio::sync::Semaphore;

use crate::config::{Secret, TektonConfig};

use super::{
    Error,
    actions::{
        RepositoryListQuery, RunCancelCommand, RunGetQuery, RunListQuery, RunRerunCommand,
        TaskListQuery, TaskLogsQuery, WorkflowDispatchCommand, WorkflowListQuery,
    },
};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_WORKFLOW_FILES: usize = 32;
const MAX_WORKFLOW_FILE_BYTES: usize = 256 * 1024;
const MAX_WORKFLOW_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_WORKFLOW_DOCUMENTS: usize = 32;
const MAX_WORKFLOW_REPOSITORIES: usize = 50;
const MAX_LOG_STEPS: usize = 8;
const MAX_KUBE_LIST_ITEMS: usize = 500;
const MAX_KUBE_LIST_PAGES: usize = 10;
const REPOSITORY_LABEL: &str = "pipelinesascode.tekton.dev/repository";
const PIPELINE_RUN_LABEL: &str = "tekton.dev/pipelineRun";

#[derive(Clone, Copy)]
enum OperationKind {
    Read,
    Mutation,
}

#[derive(Clone)]
pub struct TektonClient {
    forgejo_origin: Url,
    forgejo_token: Secret,
    pac_origin: Url,
    pac_secret: Secret,
    namespace: String,
    kube_origin: Url,
    kube_token: KubeToken,
    external: Client,
    kubernetes: Client,
    permits: Arc<Semaphore>,
    timeout: Duration,
    redactions: Arc<Vec<String>>,
}

#[derive(Clone)]
enum KubeToken {
    File(PathBuf),
    #[cfg(test)]
    Static(Secret),
}

#[derive(Clone)]
struct Repository {
    id: String,
    cr_name: String,
    owner: String,
    name: String,
    url: String,
}

#[derive(Clone)]
struct Workflow {
    id: String,
    repository: String,
    definition: String,
    path: String,
    revision: String,
    events: Vec<String>,
    triggerable: bool,
}

impl TektonClient {
    pub fn production(config: &TektonConfig, mut redactions: Vec<String>) -> Result<Self, String> {
        let host = env::var("KUBERNETES_SERVICE_HOST")
            .map_err(|_| "missing Kubernetes service host".to_owned())?;
        let port = env::var("KUBERNETES_SERVICE_PORT_HTTPS").unwrap_or_else(|_| "443".to_owned());
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        let kube_origin = Url::parse(&format!("https://{host}:{port}/"))
            .map_err(|_| "invalid Kubernetes service origin".to_owned())?;
        let token = fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/token")
            .map_err(|_| "failed to read Kubernetes service token".to_owned())?;
        let ca = fs::read("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt")
            .map_err(|_| "failed to read Kubernetes service CA".to_owned())?;
        let certificate =
            Certificate::from_pem(&ca).map_err(|_| "invalid Kubernetes service CA".to_owned())?;
        let kubernetes = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .add_root_certificate(certificate)
            .build()
            .map_err(|_| "failed to initialize Kubernetes client".to_owned())?;
        let external = safe_client()?;
        redactions.extend([
            config.forgejo_token.expose().to_owned(),
            config.pac_incoming_secret.expose().to_owned(),
            token.trim().to_owned(),
        ]);
        redactions.retain(|value| !value.is_empty());
        redactions.sort_by_key(|value| std::cmp::Reverse(value.len()));
        redactions.dedup();
        Ok(Self {
            forgejo_origin: config.forgejo_origin.clone(),
            forgejo_token: config.forgejo_token.clone(),
            pac_origin: config.pac_origin.clone(),
            pac_secret: config.pac_incoming_secret.clone(),
            namespace: config.namespace.clone(),
            kube_origin,
            kube_token: KubeToken::File(PathBuf::from(
                "/var/run/secrets/kubernetes.io/serviceaccount/token",
            )),
            external,
            kubernetes,
            permits: Arc::new(Semaphore::new(4)),
            timeout: TIMEOUT,
            redactions: Arc::new(redactions),
        })
    }

    #[cfg(test)]
    pub(crate) fn disabled_for_test() -> Self {
        let origin = Url::parse("http://127.0.0.1:9/").unwrap();
        Self {
            forgejo_origin: origin.clone(),
            forgejo_token: Secret::for_test("forgejo-secret"),
            pac_origin: origin.clone(),
            pac_secret: Secret::for_test("pac-secret"),
            namespace: "pipelines-as-code".to_owned(),
            kube_origin: origin,
            kube_token: KubeToken::Static(Secret::for_test("kube-secret")),
            external: safe_client().unwrap(),
            kubernetes: safe_client().unwrap(),
            permits: Arc::new(Semaphore::new(4)),
            timeout: Duration::from_millis(100),
            redactions: Arc::new(vec!["forgejo-secret".to_owned(), "pac-secret".to_owned()]),
        }
    }

    #[cfg(test)]
    fn for_test(origin: Url, timeout: Duration) -> Self {
        Self {
            forgejo_origin: origin.clone(),
            forgejo_token: Secret::for_test("forgejo-secret"),
            pac_origin: origin.clone(),
            pac_secret: Secret::for_test("pac-secret"),
            namespace: "pipelines-as-code".to_owned(),
            kube_origin: origin,
            kube_token: KubeToken::Static(Secret::for_test("kube-secret")),
            external: safe_client().unwrap(),
            kubernetes: safe_client().unwrap(),
            permits: Arc::new(Semaphore::new(4)),
            timeout,
            redactions: Arc::new(vec!["pac-secret".to_owned(), "kube-secret".to_owned()]),
        }
    }

    pub async fn repositories(&self, query: &RepositoryListQuery) -> Result<Value, Error> {
        let repositories = self.repository_catalog().await?;
        let truncated = repositories.len() > usize::from(query.limit);
        Ok(json!({
            "mode": "list",
            "result_type": "repositories",
            "result": repositories.into_iter().take(usize::from(query.limit)).map(repository_json).collect::<Vec<_>>(),
            "truncated": truncated,
        }))
    }

    pub async fn workflows(&self, query: &WorkflowListQuery) -> Result<Value, Error> {
        tokio::time::timeout(self.timeout, self.workflows_inner(query))
            .await
            .map_err(|_| Error::Timeout)?
    }

    async fn workflows_inner(&self, query: &WorkflowListQuery) -> Result<Value, Error> {
        let mut repositories = self.repository_catalog().await?;
        if let Some(id) = &query.repository {
            repositories.retain(|repository| &repository.id == id);
            if repositories.is_empty() {
                return Err(Error::NotFound);
            }
        }
        let mut workflows = Vec::new();
        let mut failures = Vec::new();
        let repository_truncated = repositories.len() > MAX_WORKFLOW_REPOSITORIES;
        for repository in repositories.into_iter().take(MAX_WORKFLOW_REPOSITORIES) {
            match self.repository_workflows(&repository).await {
                Ok(found) => workflows.extend(found),
                Err(_) => {
                    failures.push(json!({"repository": repository.id, "code": "discovery_failed"}))
                }
            }
        }
        workflows.sort_by(|left, right| left.id.cmp(&right.id));
        let result_truncated = workflows.len() > usize::from(query.limit);
        workflows.truncate(usize::from(query.limit));
        Ok(json!({
            "mode": "list",
            "result_type": "workflows",
            "result": workflows.into_iter().map(workflow_json).collect::<Vec<_>>(),
            "partial_failures": failures,
            "truncated": repository_truncated || result_truncated,
        }))
    }

    pub async fn runs(&self, query: &RunListQuery) -> Result<Value, Error> {
        let repository = self.repository(&query.repository).await?;
        let path = format!(
            "/apis/tekton.dev/v1/namespaces/{}/pipelineruns",
            self.namespace
        );
        let selector = format!("{REPOSITORY_LABEL}={}", repository.cr_name);
        let (items, source_truncated) = self.kube_list(&path, Some(&selector)).await?;
        let mut runs = items
            .iter()
            .filter_map(|item| normalize_run(item, &self.namespace, &repository.cr_name))
            .filter(|run| {
                query
                    .workflow
                    .as_ref()
                    .is_none_or(|value| run["workflow"] == *value)
            })
            .filter(|run| {
                query
                    .branch
                    .as_ref()
                    .is_none_or(|value| run["branch"] == *value)
            })
            .filter(|run| {
                query
                    .status
                    .as_ref()
                    .is_none_or(|value| run["status"] == *value)
            })
            .collect::<Vec<_>>();
        sort_newest(&mut runs);
        let result_truncated = runs.len() > usize::from(query.limit);
        runs.truncate(usize::from(query.limit));
        Ok(json!({
            "mode":"list", "result_type":"runs", "result":runs,
            "truncated":source_truncated || result_truncated
        }))
    }

    pub async fn run(&self, query: &RunGetQuery) -> Result<Value, Error> {
        let run = self.owned_run(&query.run_id).await?;
        let repository = label(&run, REPOSITORY_LABEL).ok_or(Error::NotFound)?;
        let normalized =
            normalize_run(&run, &self.namespace, repository).ok_or(Error::InvalidResponse)?;
        Ok(json!({"mode":"get", "result_type":"run", "result":normalized}))
    }

    pub async fn tasks(&self, query: &TaskListQuery) -> Result<Value, Error> {
        let run = self.owned_run(&query.run_id).await?;
        let run_name = object_name(&run).ok_or(Error::InvalidResponse)?;
        let run_uid = object_uid(&run).ok_or(Error::InvalidResponse)?;
        let repository = label(&run, REPOSITORY_LABEL).ok_or(Error::NotFound)?;
        let path = format!("/apis/tekton.dev/v1/namespaces/{}/taskruns", self.namespace);
        let selector = format!("{PIPELINE_RUN_LABEL}={run_name}");
        let (items, source_truncated) = self.kube_list(&path, Some(&selector)).await?;
        let mut tasks = items
            .iter()
            .filter(|item| label(item, REPOSITORY_LABEL) == Some(repository))
            .filter(|item| owned_by(item, "PipelineRun", run_name, run_uid))
            .filter_map(|item| normalize_task(item, &self.namespace, run_name))
            .collect::<Vec<_>>();
        sort_newest(&mut tasks);
        let result_truncated = tasks.len() > usize::from(query.limit);
        tasks.truncate(usize::from(query.limit));
        Ok(json!({
            "mode":"list", "result_type":"tasks", "result":tasks,
            "truncated":source_truncated || result_truncated
        }))
    }

    pub async fn logs(&self, query: &TaskLogsQuery) -> Result<Value, Error> {
        let run = self.owned_run(&query.run_id).await?;
        let run_name = object_name(&run).ok_or(Error::InvalidResponse)?;
        let run_uid = object_uid(&run).ok_or(Error::InvalidResponse)?;
        let repository = label(&run, REPOSITORY_LABEL).ok_or(Error::NotFound)?;
        let task = self.task_object(&query.task_id).await?;
        if label(&task, PIPELINE_RUN_LABEL) != Some(run_name)
            || label(&task, REPOSITORY_LABEL) != Some(repository)
            || !owned_by(&task, "PipelineRun", run_name, run_uid)
        {
            return Err(Error::NotFound);
        }
        let pod = task["status"]["podName"].as_str().ok_or(Error::NotFound)?;
        self.verify_task_pod(&task, pod).await?;
        let available = task["status"]["steps"]
            .as_array()
            .ok_or(Error::InvalidResponse)?;
        let selected_steps = available
            .iter()
            .filter_map(|step| {
                Some((
                    step["name"].as_str()?.to_owned(),
                    step["container"].as_str()?.to_owned(),
                ))
            })
            .filter(|(name, _)| query.step.as_ref().is_none_or(|selected| selected == name))
            .collect::<Vec<_>>();
        let steps_omitted = selected_steps.len() > MAX_LOG_STEPS;
        let mut steps = selected_steps
            .into_iter()
            .take(MAX_LOG_STEPS)
            .collect::<Vec<_>>();
        if steps.is_empty() {
            return Err(Error::NotFound);
        }
        let mut remaining =
            usize::try_from(query.max_bytes).map_err(|_| Error::InvalidArguments)?;
        let mut results = Vec::new();
        for (name, container) in steps.drain(..) {
            if remaining == 0 {
                break;
            }
            let path = format!("/api/v1/namespaces/{}/pods/{pod}/log", self.namespace);
            let requested = remaining;
            let (bytes, kube_token) = self
                .kube_text(
                    &path,
                    &[
                        ("container", container),
                        ("tailLines", query.tail_lines.to_string()),
                        ("limitBytes", requested.to_string()),
                    ],
                    requested,
                )
                .await?;
            let byte_truncated = bytes.len() >= requested;
            let mut text = String::from_utf8_lossy(&bytes).into_owned();
            for secret in self.redactions.iter() {
                text = text.replace(secret, "[REDACTED]");
            }
            text = text.replace(kube_token.expose(), "[REDACTED]");
            let redaction_truncated = truncate_utf8(&mut text, requested);
            remaining = remaining.saturating_sub(text.len());
            let tail_truncated = text.lines().count() >= usize::from(query.tail_lines);
            results.push(json!({
                "step": name,
                "text": text,
                "tail_truncated": tail_truncated,
                "byte_truncated": byte_truncated || redaction_truncated,
            }));
        }
        Ok(json!({
            "mode":"logs", "result_type":"task_logs", "run_id":query.run_id,
            "task_id":query.task_id, "tail_lines":query.tail_lines,
            "max_bytes":query.max_bytes, "result":results,
            "truncated":steps_omitted || remaining == 0 || results.iter().any(|value| value["tail_truncated"] == true || value["byte_truncated"] == true),
            "steps_omitted":steps_omitted,
        }))
    }

    pub async fn dispatch(&self, command: &WorkflowDispatchCommand) -> Result<Value, Error> {
        let repository = self.repository(&command.repository).await?;
        let workflow = self
            .triggerable_workflow(&repository, &command.workflow)
            .await?;
        self.send_pac(
            &repository,
            &workflow.definition,
            &command.reference,
            &command.params,
        )
        .await?;
        Ok(
            json!({"status":"accepted", "repository":repository.id, "workflow":workflow.id, "ref":command.reference}),
        )
    }

    pub async fn rerun(&self, command: &RunRerunCommand) -> Result<Value, Error> {
        let run = self.owned_run(&command.run_id).await?;
        let repository_name = label(&run, REPOSITORY_LABEL).ok_or(Error::NotFound)?;
        let repository = self
            .repository(&format!("{}/{}", self.namespace, repository_name))
            .await?;
        let definition = workflow_name(&run).ok_or(Error::MutationRejected)?;
        let workflow = self
            .repository_workflows(&repository)
            .await?
            .into_iter()
            .find(|workflow| workflow.definition == definition && workflow.triggerable)
            .ok_or(Error::MutationRejected)?;
        let reference = annotation(&run, "pipelinesascode.tekton.dev/branch")
            .or_else(|| annotation(&run, "pipelinesascode.tekton.dev/source-branch"))
            .ok_or(Error::MutationRejected)?;
        let params = run["spec"]["params"]
            .as_array()
            .ok_or(Error::MutationRejected)?
            .iter()
            .map(|param| {
                let name = param["name"].as_str().ok_or(Error::MutationRejected)?;
                let value = param["value"].as_str().ok_or(Error::MutationRejected)?;
                Ok((name.to_owned(), value.to_owned()))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        if !valid_replay(reference, &params) {
            return Err(Error::MutationRejected);
        }
        self.send_pac(&repository, &workflow.definition, reference, &params)
            .await?;
        Ok(
            json!({"status":"accepted", "run_id":command.run_id, "repository":repository.id, "workflow":workflow.id}),
        )
    }

    pub async fn cancel(&self, command: &RunCancelCommand) -> Result<Value, Error> {
        let run = self.owned_run(&command.run_id).await?;
        if run_status(&run) != "running" {
            return Err(Error::MutationRejected);
        }
        let resource_version = run["metadata"]["resourceVersion"]
            .as_str()
            .ok_or(Error::MutationRejected)?;
        let (_, name) = namespaced_id(&command.run_id, &self.namespace)?;
        let path = format!(
            "/apis/tekton.dev/v1/namespaces/{}/pipelineruns/{name}",
            self.namespace
        );
        self.kube_json(
            Method::PATCH,
            &path,
            &[],
            Some(cancel_patch(resource_version)),
            OperationKind::Mutation,
        )
        .await?;
        Ok(json!({"status":"cancellation_requested", "run_id":command.run_id}))
    }

    async fn repository_catalog(&self) -> Result<Vec<Repository>, Error> {
        let path = format!(
            "/apis/pipelinesascode.tekton.dev/v1alpha1/namespaces/{}/repositories",
            self.namespace
        );
        let (items, truncated) = self.kube_list(&path, None).await?;
        if truncated {
            return Err(Error::InvalidResponse);
        }
        let mut repositories = items
            .iter()
            .filter_map(|item| parse_repository(item, &self.namespace, &self.forgejo_origin))
            .collect::<Vec<_>>();
        repositories.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(repositories)
    }

    async fn repository(&self, id: &str) -> Result<Repository, Error> {
        self.repository_catalog()
            .await?
            .into_iter()
            .find(|repository| repository.id == id)
            .ok_or(Error::NotFound)
    }

    async fn repository_workflows(&self, repository: &Repository) -> Result<Vec<Workflow>, Error> {
        let metadata = self
            .forgejo_json(
                &["api", "v1", "repos", &repository.owner, &repository.name],
                &[],
            )
            .await?;
        let revision = metadata["default_branch"]
            .as_str()
            .ok_or(Error::InvalidResponse)?
            .to_owned();
        if !bounded_text(&revision, 512) {
            return Err(Error::InvalidResponse);
        }
        let listing = self
            .forgejo_json(
                &[
                    "api",
                    "v1",
                    "repos",
                    &repository.owner,
                    &repository.name,
                    "contents",
                    ".tekton",
                ],
                &[("ref", revision.clone())],
            )
            .await?;
        let files = listing
            .as_array()
            .ok_or(Error::InvalidResponse)?
            .iter()
            .filter(|entry| entry["type"] == "file")
            .filter_map(|entry| entry["path"].as_str())
            .filter(|path| {
                path.starts_with(".tekton/")
                    && path.len() <= 512
                    && (path.ends_with(".yaml") || path.ends_with(".yml"))
            })
            .collect::<Vec<_>>();
        if files.len() > MAX_WORKFLOW_FILES {
            return Err(Error::InvalidResponse);
        }
        let mut workflows = Vec::new();
        let mut total_bytes = 0usize;
        for entry in files {
            let mut segments = vec![
                "api",
                "v1",
                "repos",
                repository.owner.as_str(),
                repository.name.as_str(),
                "contents",
            ];
            segments.extend(entry.split('/'));
            let content = self
                .forgejo_json(&segments, &[("ref", revision.clone())])
                .await?;
            if content["encoding"] != "base64" {
                return Err(Error::InvalidResponse);
            }
            let encoded = content["content"].as_str().ok_or(Error::InvalidResponse)?;
            let bytes = STANDARD
                .decode(encoded.replace('\n', ""))
                .map_err(|_| Error::InvalidResponse)?;
            if bytes.len() > MAX_WORKFLOW_FILE_BYTES {
                return Err(Error::InvalidResponse);
            }
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .ok_or(Error::InvalidResponse)?;
            if total_bytes > MAX_WORKFLOW_TOTAL_BYTES {
                return Err(Error::InvalidResponse);
            }
            workflows.extend(parse_workflows(repository, &revision, entry, &bytes)?);
        }
        Ok(workflows)
    }

    async fn triggerable_workflow(
        &self,
        repository: &Repository,
        id: &str,
    ) -> Result<Workflow, Error> {
        self.repository_workflows(repository)
            .await?
            .into_iter()
            .find(|workflow| workflow.id == id && workflow.triggerable)
            .ok_or(Error::MutationRejected)
    }

    async fn owned_run(&self, id: &str) -> Result<Value, Error> {
        let (_, name) = namespaced_id(id, &self.namespace)?;
        let path = format!(
            "/apis/tekton.dev/v1/namespaces/{}/pipelineruns/{name}",
            self.namespace
        );
        let run = self
            .kube_json(Method::GET, &path, &[], None, OperationKind::Read)
            .await?;
        let repository = label(&run, REPOSITORY_LABEL).ok_or(Error::NotFound)?;
        self.repository(&format!("{}/{}", self.namespace, repository))
            .await?;
        Ok(run)
    }

    async fn task_object(&self, id: &str) -> Result<Value, Error> {
        let (_, name) = namespaced_id(id, &self.namespace)?;
        let path = format!(
            "/apis/tekton.dev/v1/namespaces/{}/taskruns/{name}",
            self.namespace
        );
        self.kube_json(Method::GET, &path, &[], None, OperationKind::Read)
            .await
    }

    async fn verify_task_pod(&self, task: &Value, pod_name: &str) -> Result<(), Error> {
        let task_uid = task["metadata"]["uid"].as_str().ok_or(Error::NotFound)?;
        let task_name = object_name(task).ok_or(Error::NotFound)?;
        let path = format!("/api/v1/namespaces/{}/pods/{pod_name}", self.namespace);
        let pod = self
            .kube_json(Method::GET, &path, &[], None, OperationKind::Read)
            .await?;
        let owned = pod["metadata"]["ownerReferences"]
            .as_array()
            .is_some_and(|owners| {
                owners.iter().any(|owner| {
                    owner["kind"] == "TaskRun"
                        && owner["name"] == task_name
                        && owner["uid"] == task_uid
                })
            });
        if !owned {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    async fn forgejo_json(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
    ) -> Result<Value, Error> {
        let mut url = self.forgejo_origin.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| Error::InvalidArguments)?;
            path.clear();
            path.extend(segments.iter().copied());
        }
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (key, value)));
        self.http_json(
            self.external
                .get(url)
                .bearer_auth(self.forgejo_token.expose()),
            OperationKind::Read,
        )
        .await
    }

    async fn send_pac(
        &self,
        repository: &Repository,
        definition: &str,
        reference: &str,
        params: &BTreeMap<String, String>,
    ) -> Result<(), Error> {
        let url = self
            .pac_origin
            .join("incoming")
            .map_err(|_| Error::MutationRejected)?;
        let body = json!({
            "repository": repository.cr_name,
            "namespace": self.namespace,
            "branch": reference,
            "pipelinerun": definition,
            "secret": self.pac_secret.expose(),
            "params": params,
        });
        self.http_empty(self.external.post(url).json(&body), OperationKind::Mutation)
            .await
    }

    async fn kube_json(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
        kind: OperationKind,
    ) -> Result<Value, Error> {
        let mut url = self
            .kube_origin
            .join(path.trim_start_matches('/'))
            .map_err(|_| operation_failure(kind))?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (key, value)));
        let mut builder = self
            .kubernetes
            .request(method.clone(), url)
            .bearer_auth(self.kube_token().await?.expose());
        if let Some(body) = body {
            builder = builder
                .header("Content-Type", "application/merge-patch+json")
                .json(&body);
        }
        if method == Method::PATCH {
            self.http_empty(builder, kind).await?;
            return Ok(json!({}));
        }
        self.http_json(builder, kind).await
    }

    async fn kube_list(
        &self,
        path: &str,
        selector: Option<&str>,
    ) -> Result<(Vec<Value>, bool), Error> {
        let mut items = Vec::new();
        let mut continuation: Option<String> = None;
        let mut seen_continuations = HashSet::new();
        let mut pages = 0usize;
        loop {
            let remaining = MAX_KUBE_LIST_ITEMS - items.len();
            if remaining == 0 {
                return Ok((items, continuation.is_some()));
            }
            let mut query = vec![("limit", remaining.min(100).to_string())];
            if let Some(selector) = selector {
                query.push(("labelSelector", selector.to_owned()));
            }
            if let Some(token) = continuation.as_ref() {
                query.push(("continue", token.clone()));
            }
            let wrapper = self
                .kube_json(Method::GET, path, &query, None, OperationKind::Read)
                .await?;
            pages += 1;
            let page = wrapper["items"].as_array().ok_or(Error::InvalidResponse)?;
            items.extend(page.iter().take(remaining).cloned());
            continuation = wrapper["metadata"]["continue"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            if continuation.is_none() {
                return Ok((items, false));
            }
            let token = continuation.as_ref().expect("checked as present");
            if !seen_continuations.insert(token.clone()) {
                return Err(Error::InvalidResponse);
            }
            if pages >= MAX_KUBE_LIST_PAGES {
                return Ok((items, true));
            }
        }
    }

    async fn kube_text(
        &self,
        path: &str,
        query: &[(&str, String)],
        limit: usize,
    ) -> Result<(Vec<u8>, Secret), Error> {
        let mut url = self
            .kube_origin
            .join(path.trim_start_matches('/'))
            .map_err(|_| Error::UpstreamUnavailable)?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (key, value)));
        let token = self.kube_token().await?;
        let bytes = self
            .http_bytes(
                self.kubernetes.get(url).bearer_auth(token.expose()),
                OperationKind::Read,
                limit,
            )
            .await?;
        Ok((bytes, token))
    }

    async fn kube_token(&self) -> Result<Secret, Error> {
        match &self.kube_token {
            KubeToken::File(path) => tokio::fs::read_to_string(path)
                .await
                .map_err(|_| Error::UpstreamUnavailable)
                .and_then(|value| {
                    let value = value.trim();
                    if value.is_empty() {
                        Err(Error::UpstreamUnavailable)
                    } else {
                        Ok(Secret(value.to_owned()))
                    }
                }),
            #[cfg(test)]
            KubeToken::Static(value) => Ok(value.clone()),
        }
    }

    async fn http_json(
        &self,
        builder: reqwest::RequestBuilder,
        kind: OperationKind,
    ) -> Result<Value, Error> {
        let bytes = self.http_bytes(builder, kind, MAX_RESPONSE_BYTES).await?;
        serde_json::from_slice(&bytes).map_err(|_| match kind {
            OperationKind::Read => Error::InvalidResponse,
            OperationKind::Mutation => Error::MutationOutcomeUnknown,
        })
    }

    async fn http_empty(
        &self,
        builder: reqwest::RequestBuilder,
        kind: OperationKind,
    ) -> Result<(), Error> {
        self.http_bytes(builder, kind, MAX_RESPONSE_BYTES)
            .await
            .map(|_| ())
    }

    async fn http_bytes(
        &self,
        builder: reqwest::RequestBuilder,
        kind: OperationKind,
        limit: usize,
    ) -> Result<Vec<u8>, Error> {
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::CapacityExhausted)?;
        let operation = async {
            let mut response = builder.send().await.map_err(|_| operation_failure(kind))?;
            let status = response.status();
            if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                return Err(Error::Unauthorized);
            }
            if status == StatusCode::NOT_FOUND {
                return Err(Error::NotFound);
            }
            if !status.is_success() {
                return Err(match kind {
                    OperationKind::Read if status.is_client_error() => Error::QueryRejected,
                    OperationKind::Read => Error::UpstreamUnavailable,
                    OperationKind::Mutation if status.is_client_error() => Error::MutationRejected,
                    OperationKind::Mutation => Error::MutationOutcomeUnknown,
                });
            }
            if response
                .content_length()
                .is_some_and(|length| length > limit as u64)
            {
                return Err(match kind {
                    OperationKind::Read => Error::InvalidResponse,
                    OperationKind::Mutation => Error::MutationOutcomeUnknown,
                });
            }
            let mut bytes = Vec::new();
            loop {
                let chunk = response
                    .chunk()
                    .await
                    .map_err(|_| operation_failure(kind))?;
                let Some(chunk) = chunk else { break };
                if chunk.len() > limit - bytes.len() {
                    return Err(match kind {
                        OperationKind::Read => Error::InvalidResponse,
                        OperationKind::Mutation => Error::MutationOutcomeUnknown,
                    });
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        };
        let result = tokio::time::timeout(self.timeout, operation)
            .await
            .map_err(|_| match kind {
                OperationKind::Read => Error::Timeout,
                OperationKind::Mutation => Error::MutationOutcomeUnknown,
            })?;
        drop(permit);
        result
    }
}

fn safe_client() -> Result<Client, String> {
    Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| "failed to initialize Tekton integration client".to_owned())
}

fn operation_failure(kind: OperationKind) -> Error {
    match kind {
        OperationKind::Read => Error::UpstreamUnavailable,
        OperationKind::Mutation => Error::MutationOutcomeUnknown,
    }
}

fn parse_repository(item: &Value, namespace: &str, origin: &Url) -> Option<Repository> {
    let cr_name = object_name(item)?.to_owned();
    let raw = item["spec"]["url"].as_str()?;
    let url = Url::parse(raw).ok()?;
    if url.scheme() != origin.scheme()
        || url.host_str() != origin.host_str()
        || url.port_or_known_default() != origin.port_or_known_default()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let parts = url
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() != 2 {
        return None;
    }
    Some(Repository {
        id: format!("{namespace}/{cr_name}"),
        cr_name,
        owner: parts[0].to_owned(),
        name: parts[1].trim_end_matches(".git").to_owned(),
        url: format!(
            "{}/{}/{}",
            origin.as_str().trim_end_matches('/'),
            parts[0],
            parts[1].trim_end_matches(".git")
        ),
    })
}

fn repository_json(repository: Repository) -> Value {
    json!({"id":repository.id, "owner":repository.owner, "repository":repository.name, "url":repository.url})
}

fn parse_workflows(
    repository: &Repository,
    revision: &str,
    path: &str,
    bytes: &[u8],
) -> Result<Vec<Workflow>, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidResponse)?;
    let mut workflows = Vec::new();
    for (index, document) in serde_yaml::Deserializer::from_str(text).enumerate() {
        if index >= MAX_WORKFLOW_DOCUMENTS {
            return Err(Error::InvalidResponse);
        }
        let value = serde_yaml::Value::deserialize(document).map_err(|_| Error::InvalidResponse)?;
        let value = serde_json::to_value(value).map_err(|_| Error::InvalidResponse)?;
        if value["kind"] != "PipelineRun" {
            continue;
        }
        let definition = value["metadata"]["name"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                value["metadata"]["generateName"]
                    .as_str()
                    .map(|value| value.trim_end_matches('-').to_owned())
            })
            .filter(|value| !value.is_empty())
            .ok_or(Error::InvalidResponse)?;
        if !bounded_text(&definition, 253) {
            return Err(Error::InvalidResponse);
        }
        let annotation = value["metadata"]["annotations"]["pipelinesascode.tekton.dev/on-event"]
            .as_str()
            .or_else(|| {
                value["metadata"]["annotations"]["pipelinesascode.tekton.dev/event"].as_str()
            })
            .unwrap_or("");
        let mut events = annotation
            .trim_matches(|character| matches!(character, '[' | ']'))
            .split(',')
            .map(str::trim)
            .map(|event| event.trim_matches(['\'', '"']))
            .filter(|event| !event.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if events.len() > 20 || events.iter().any(|event| !bounded_text(event, 64)) {
            return Err(Error::InvalidResponse);
        }
        events.sort();
        events.dedup();
        let triggerable = events.iter().any(|event| event == "incoming");
        let digest = Sha256::digest(format!(
            "{}\0{revision}\0{path}\0{index}\0{definition}",
            repository.id
        ));
        let id = format!("workflow/{}", URL_SAFE_NO_PAD.encode(digest));
        workflows.push(Workflow {
            id,
            repository: repository.id.clone(),
            definition,
            path: path.to_owned(),
            revision: revision.to_owned(),
            events,
            triggerable,
        });
    }
    Ok(workflows)
}

fn workflow_json(workflow: Workflow) -> Value {
    json!({
        "id":workflow.id, "repository":workflow.repository, "name":workflow.definition,
        "path":workflow.path, "revision":workflow.revision, "events":workflow.events,
        "triggerable":workflow.triggerable,
    })
}

fn normalize_run(item: &Value, namespace: &str, repository: &str) -> Option<Value> {
    if label(item, REPOSITORY_LABEL)? != repository {
        return None;
    }
    let name = object_name(item)?;
    let parameter_names = item["spec"]["params"]
        .as_array()
        .map(|params| {
            params
                .iter()
                .filter_map(|param| param["name"].as_str())
                .take(32)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(json!({
        "id":format!("{namespace}/{name}"),
        "repository":format!("{namespace}/{repository}"),
        "workflow":workflow_name(item).unwrap_or(name),
        "branch":annotation(item, "pipelinesascode.tekton.dev/branch").or_else(|| annotation(item, "pipelinesascode.tekton.dev/source-branch")),
        "revision":label(item, "pipelinesascode.tekton.dev/sha"),
        "status":run_status(item),
        "reason":condition(item).and_then(|value| value["reason"].as_str()),
        "created_at":item["metadata"]["creationTimestamp"].as_str(),
        "started_at":item["status"]["startTime"].as_str(),
        "completed_at":item["status"]["completionTime"].as_str(),
        "parameter_names":parameter_names,
    }))
}

fn normalize_task(item: &Value, namespace: &str, run_name: &str) -> Option<Value> {
    let name = object_name(item)?;
    let steps = item["status"]["steps"]
        .as_array()
        .map(|steps| {
            steps.iter().filter_map(|step| {
                Some(json!({
                    "name":step["name"].as_str()?,
                    "status":if step.get("terminated").is_some() { "terminated" } else if step.get("running").is_some() { "running" } else { "waiting" },
                    "reason":step["terminated"]["reason"].as_str().or_else(|| step["waiting"]["reason"].as_str()),
                }))
            }).take(32).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(json!({
        "id":format!("{namespace}/{name}"), "run_id":format!("{namespace}/{run_name}"),
        "pipeline_task":label(item, "tekton.dev/pipelineTask"),
        "status":run_status(item), "reason":condition(item).and_then(|value| value["reason"].as_str()),
        "created_at":item["metadata"]["creationTimestamp"].as_str(),
        "started_at":item["status"]["startTime"].as_str(),
        "completed_at":item["status"]["completionTime"].as_str(), "steps":steps,
    }))
}

fn run_status(item: &Value) -> &'static str {
    let Some(condition) = condition(item) else {
        return "running";
    };
    match (condition["status"].as_str(), condition["reason"].as_str()) {
        (Some("True"), _) => "succeeded",
        (Some("False"), Some(reason)) if reason.to_ascii_lowercase().contains("cancel") => {
            "cancelled"
        }
        (Some("False"), _) => "failed",
        _ => "running",
    }
}

fn condition(item: &Value) -> Option<&Value> {
    item["status"]["conditions"]
        .as_array()?
        .iter()
        .find(|condition| condition["type"] == "Succeeded")
}

fn workflow_name(item: &Value) -> Option<&str> {
    label(item, "pipelinesascode.tekton.dev/original-prname")
        .or_else(|| annotation(item, "pipelinesascode.tekton.dev/original-prname"))
}

fn object_name(item: &Value) -> Option<&str> {
    item["metadata"]["name"].as_str()
}

fn object_uid(item: &Value) -> Option<&str> {
    item["metadata"]["uid"].as_str()
}

fn owned_by(item: &Value, kind: &str, name: &str, uid: &str) -> bool {
    item["metadata"]["ownerReferences"]
        .as_array()
        .is_some_and(|owners| {
            owners
                .iter()
                .any(|owner| owner["kind"] == kind && owner["name"] == name && owner["uid"] == uid)
        })
}

fn label<'a>(item: &'a Value, name: &str) -> Option<&'a str> {
    item["metadata"]["labels"][name].as_str()
}

fn annotation<'a>(item: &'a Value, name: &str) -> Option<&'a str> {
    item["metadata"]["annotations"][name].as_str()
}

fn namespaced_id<'a>(id: &'a str, expected_namespace: &str) -> Result<(&'a str, &'a str), Error> {
    let (namespace, name) = id.split_once('/').ok_or(Error::InvalidArguments)?;
    if namespace != expected_namespace || name.is_empty() || name.contains('/') {
        return Err(Error::InvalidArguments);
    }
    Ok((namespace, name))
}

fn sort_newest(values: &mut [Value]) {
    values.sort_by(|left, right| {
        right["created_at"]
            .as_str()
            .cmp(&left["created_at"].as_str())
            .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
    });
}

fn truncate_utf8(value: &mut String, maximum: usize) -> bool {
    if value.len() <= maximum {
        return false;
    }
    let mut boundary = maximum;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    true
}

fn valid_replay(reference: &str, params: &BTreeMap<String, String>) -> bool {
    !reference.trim().is_empty()
        && reference.len() <= 512
        && !reference.chars().any(char::is_control)
        && params.len() <= 20
        && params.iter().all(|(key, value)| {
            !key.trim().is_empty()
                && key.len() <= 128
                && value.len() <= 4096
                && !key.chars().any(char::is_control)
                && !value.chars().any(char::is_control)
        })
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn cancel_patch(resource_version: &str) -> Value {
    json!({
        "metadata":{"resourceVersion":resource_version},
        "spec":{"status":"Cancelled"}
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use axum::{
        Json, Router,
        body::{Body, to_bytes},
        extract::State,
        http::{Request, StatusCode},
        response::{IntoResponse, Response},
    };
    use tokio::{net::TcpListener, task::JoinHandle};

    use super::*;

    struct MockState {
        repository_url: String,
        pac_bodies: Mutex<Vec<Value>>,
        cancel_bodies: Mutex<Vec<Value>>,
        stall_pac: AtomicBool,
        repeat_page: AtomicBool,
        conflict_cancel: AtomicBool,
    }

    async fn mock_upstreams(
        State(state): State<Arc<MockState>>,
        request: Request<Body>,
    ) -> Response {
        let path = request.uri().path().to_owned();
        match (request.method().as_str(), path.as_str()) {
            (
                "GET",
                "/apis/pipelinesascode.tekton.dev/v1alpha1/namespaces/pipelines-as-code/repositories",
            ) => {
                if state.repeat_page.load(Ordering::SeqCst) {
                    Json(json!({"items":[], "metadata":{"continue":"repeat"}})).into_response()
                } else {
                    Json(json!({
                        "items":[{"metadata":{"name":"pac-rfhold-repo"},"spec":{"url":state.repository_url}}],
                        "metadata":{}
                    })).into_response()
                }
            }
            ("GET", "/apis/tekton.dev/v1/namespaces/pipelines-as-code/pipelineruns/run") => {
                Json(json!({
                    "metadata":{
                        "name":"run", "uid":"run-uid", "resourceVersion":"rv-1",
                        "labels":{"pipelinesascode.tekton.dev/repository":"pac-rfhold-repo"}
                    },
                    "status":{"conditions":[{"type":"Succeeded","status":"Unknown"}]}
                }))
                .into_response()
            }
            ("PATCH", "/apis/tekton.dev/v1/namespaces/pipelines-as-code/pipelineruns/run") => {
                let body = to_bytes(request.into_body(), 16 * 1024).await.unwrap();
                state
                    .cancel_bodies
                    .lock()
                    .unwrap()
                    .push(serde_json::from_slice(&body).unwrap());
                if state.conflict_cancel.load(Ordering::SeqCst) {
                    StatusCode::CONFLICT.into_response()
                } else {
                    StatusCode::OK.into_response()
                }
            }
            ("GET", "/api/v1/repos/rfhold/repo") => {
                Json(json!({"default_branch":"main"})).into_response()
            }
            ("GET", "/api/v1/repos/rfhold/repo/contents/.tekton") => Json(json!([
                {"type":"file","path":".tekton/build.yaml"}
            ]))
            .into_response(),
            ("GET", "/api/v1/repos/rfhold/repo/contents/.tekton/build.yaml") => {
                let content = STANDARD.encode(
                    b"kind: PipelineRun\nmetadata:\n  name: build\n  annotations:\n    pipelinesascode.tekton.dev/on-event: '[incoming]'\n",
                );
                Json(json!({"encoding":"base64","content":content})).into_response()
            }
            ("POST", "/incoming") => {
                let body = to_bytes(request.into_body(), 16 * 1024).await.unwrap();
                state
                    .pac_bodies
                    .lock()
                    .unwrap()
                    .push(serde_json::from_slice(&body).unwrap());
                if state.stall_pac.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                StatusCode::ACCEPTED.into_response()
            }
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    }

    async fn mock_server() -> (Url, Arc<MockState>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let state = Arc::new(MockState {
            repository_url: format!("{}rfhold/repo", origin.as_str()),
            pac_bodies: Mutex::new(Vec::new()),
            cancel_bodies: Mutex::new(Vec::new()),
            stall_pac: AtomicBool::new(false),
            repeat_page: AtomicBool::new(false),
            conflict_cancel: AtomicBool::new(false),
        });
        let server_state = state.clone();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .fallback(mock_upstreams)
                    .with_state(server_state),
            )
            .await
            .unwrap();
        });
        (origin, state, task)
    }

    #[test]
    fn repository_parser_excludes_non_repositories_and_foreign_origins() {
        let origin = Url::parse("https://git.example/").unwrap();
        assert!(
            parse_repository(
                &json!({"metadata":{"name":"global"},"spec":{}}),
                "pipelines-as-code",
                &origin
            )
            .is_none()
        );
        assert!(parse_repository(&json!({"metadata":{"name":"foreign"},"spec":{"url":"https://other.example/rfhold/repo"}}), "pipelines-as-code", &origin).is_none());
        let repository = parse_repository(&json!({"metadata":{"name":"pac-rfhold-repo"},"spec":{"url":"https://git.example/rfhold/repo"}}), "pipelines-as-code", &origin).unwrap();
        assert_eq!(repository.id, "pipelines-as-code/pac-rfhold-repo");
        assert_eq!(repository.name, "repo");
    }

    #[test]
    fn workflow_parser_marks_only_exact_incoming_event_triggerable() {
        let repository = Repository {
            id: "pipelines-as-code/pac-rfhold-repo".to_owned(),
            cr_name: "pac-rfhold-repo".to_owned(),
            owner: "rfhold".to_owned(),
            name: "repo".to_owned(),
            url: "https://git.example/rfhold/repo".to_owned(),
        };
        let workflows = parse_workflows(
            &repository,
            "main",
            ".tekton/build.yaml",
            br#"
apiVersion: tekton.dev/v1
kind: PipelineRun
metadata:
  name: build
  annotations:
    pipelinesascode.tekton.dev/on-event: "[push, incoming-extra, incoming]"
spec: {}
"#,
        )
        .unwrap();
        assert_eq!(workflows.len(), 1);
        assert!(workflows[0].triggerable);
        assert_eq!(
            workflows[0].events,
            vec!["incoming", "incoming-extra", "push"]
        );
    }

    #[test]
    fn normalization_uses_exact_ownership_and_hides_parameter_values() {
        let run = json!({
            "metadata":{"name":"run","labels":{"pipelinesascode.tekton.dev/repository":"pac-rfhold-repo","pipelinesascode.tekton.dev/sha":"abc"},"creationTimestamp":"2026-01-01T00:00:00Z"},
            "spec":{"params":[{"name":"token","value":"secret-value"}]},
            "status":{"conditions":[{"type":"Succeeded","status":"True","reason":"Succeeded"}]}
        });
        let normalized = normalize_run(&run, "pipelines-as-code", "pac-rfhold-repo").unwrap();
        assert_eq!(normalized["parameter_names"], json!(["token"]));
        assert!(!normalized.to_string().contains("secret-value"));
        assert!(normalize_run(&run, "pipelines-as-code", "different").is_none());
    }

    #[test]
    fn ownership_and_cancel_precondition_are_exact() {
        let object = json!({"metadata":{"ownerReferences":[{
            "kind":"PipelineRun", "name":"run", "uid":"uid-1"
        }]}});
        assert!(owned_by(&object, "PipelineRun", "run", "uid-1"));
        assert!(!owned_by(&object, "PipelineRun", "run", "uid-2"));
        assert_eq!(
            cancel_patch("rv-1"),
            json!({"metadata":{"resourceVersion":"rv-1"},"spec":{"status":"Cancelled"}})
        );
    }

    #[tokio::test]
    async fn dispatch_sends_exact_pac_body_once_and_marks_timeout_uncertain() {
        let (origin, state, server) = mock_server().await;
        let client = TektonClient::for_test(origin, Duration::from_millis(50));
        let catalog = client
            .workflows(&WorkflowListQuery {
                repository: Some("pipelines-as-code/pac-rfhold-repo".to_owned()),
                limit: 10,
            })
            .await
            .unwrap();
        let workflow = catalog["result"][0]["id"].as_str().unwrap().to_owned();
        let command = WorkflowDispatchCommand {
            repository: "pipelines-as-code/pac-rfhold-repo".to_owned(),
            workflow,
            reference: "main".to_owned(),
            params: BTreeMap::from([("image".to_owned(), "example".to_owned())]),
        };
        let result = client.dispatch(&command).await.unwrap();
        assert_eq!(result["status"], "accepted");
        assert_eq!(
            state.pac_bodies.lock().unwrap().as_slice(),
            &[json!({
                "repository":"pac-rfhold-repo", "namespace":"pipelines-as-code",
                "branch":"main", "pipelinerun":"build", "secret":"pac-secret",
                "params":{"image":"example"}
            })]
        );

        state.stall_pac.store(true, Ordering::SeqCst);
        assert_eq!(
            client.dispatch(&command).await,
            Err(Error::MutationOutcomeUnknown)
        );
        assert_eq!(state.pac_bodies.lock().unwrap().len(), 2);
        server.abort();
    }

    #[tokio::test]
    async fn cancellation_uses_resource_version_and_conflict_is_rejected() {
        let (origin, state, server) = mock_server().await;
        let client = TektonClient::for_test(origin, Duration::from_secs(1));
        let command = RunCancelCommand {
            run_id: "pipelines-as-code/run".to_owned(),
        };
        assert_eq!(
            client.cancel(&command).await.unwrap()["status"],
            "cancellation_requested"
        );
        assert_eq!(
            state.cancel_bodies.lock().unwrap()[0],
            json!({"metadata":{"resourceVersion":"rv-1"},"spec":{"status":"Cancelled"}})
        );
        state.conflict_cancel.store(true, Ordering::SeqCst);
        assert_eq!(client.cancel(&command).await, Err(Error::MutationRejected));
        server.abort();
    }

    #[tokio::test]
    async fn repeated_kubernetes_continuation_is_rejected() {
        let (origin, state, server) = mock_server().await;
        state.repeat_page.store(true, Ordering::SeqCst);
        let client = TektonClient::for_test(origin, Duration::from_secs(1));
        assert!(matches!(
            client.repository_catalog().await,
            Err(Error::InvalidResponse)
        ));
        server.abort();
    }

    #[tokio::test]
    async fn projected_token_is_reloaded_from_disk() {
        let path = std::env::temp_dir().join(format!(
            "homelab-mcp-token-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        tokio::fs::write(&path, "first").await.unwrap();
        let mut client = TektonClient::disabled_for_test();
        client.kube_token = KubeToken::File(path.clone());
        assert_eq!(client.kube_token().await.unwrap().expose(), "first");
        tokio::fs::write(&path, "second").await.unwrap();
        assert_eq!(client.kube_token().await.unwrap().expose(), "second");
        tokio::fs::remove_file(path).await.unwrap();
    }
}
