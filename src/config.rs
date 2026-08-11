use std::{
    collections::HashSet,
    env, fs,
    path::{Component, Path},
    sync::Arc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use mcp::VersionedOAuthWrappingKeyring;
use serde::Deserialize;
use url::Url;

const PREFIX: &str = "HOMELAB_MCP_";
const REQUIRED_OAUTH_SCOPES: [&str; 3] = ["mcp:use", "kubernetes:read", "kubernetes:write"];
const MAX_KUBERNETES_CLUSTERS_JSON_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelemetryConfig {
    pub deployment_environment: String,
    pub k8s_namespace: Option<String>,
    pub k8s_pod_name: Option<String>,
    pub k8s_pod_uid: Option<String>,
    pub pyroscope_url: Option<Url>,
}

impl TelemetryConfig {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            deployment_environment: required("DEPLOYMENT_ENVIRONMENT")?,
            k8s_namespace: optional("K8S_NAMESPACE"),
            k8s_pod_name: optional("K8S_POD_NAME"),
            k8s_pod_uid: optional("K8S_POD_UID"),
            pyroscope_url: optional("PYROSCOPE_URL")
                .map(|value| secure_origin("PYROSCOPE_URL", &value))
                .transpose()?,
        })
    }
}

#[derive(Clone)]
pub struct Config {
    pub database: DatabaseConfig,
    pub oidc: OidcConfig,
    pub oauth: OAuthConfig,
    pub integrations: IntegrationsConfig,
}

#[derive(Clone)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Clone)]
pub struct OidcConfig {
    pub public_url: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Secret,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

#[derive(Clone)]
pub struct OAuthConfig {
    pub issuer: String,
    pub resource: String,
    pub required_scopes: Vec<String>,
    pub access_token_ttl: Duration,
    pub refresh_token_ttl: Duration,
    pub refresh_family_ttl: Duration,
    pub code_ttl: Duration,
    pub allow_dcr: bool,
    pub allow_cimd: bool,
    pub cimd_trusted_private_origins: Vec<Url>,
    pub allow_loopback_redirects: bool,
    pub wrapping_keys_file: String,
}

#[derive(Clone)]
pub struct IntegrationsConfig {
    pub grafana: GrafanaConfig,
    pub tekton: TektonConfig,
    pub kubernetes: KubernetesIntegrationConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KubernetesIntegrationConfig {
    pub kubectl_path: String,
    pub clusters: Vec<KubernetesClusterConfig>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KubernetesClusterConfig {
    pub name: String,
    pub kubeconfig: String,
    pub context: String,
    pub cache_dir: String,
}

#[derive(Clone)]
pub struct GrafanaConfig {
    pub origin: Url,
    pub token: Secret,
}

#[derive(Clone)]
pub struct TektonConfig {
    pub forgejo_origin: Url,
    pub forgejo_token: Secret,
    pub namespace: String,
    pub pac_origin: Url,
    pub pac_incoming_secret: Secret,
}

#[derive(Clone)]
pub struct Secret(pub(crate) String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyringFile {
    schema_version: u8,
    active: String,
    keys: Vec<KeyringKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyringKey {
    id: String,
    key: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let config = Self {
            database: DatabaseConfig {
                url: required("DATABASE_URL")?,
            },
            oidc: OidcConfig {
                public_url: required("PUBLIC_URL")?,
                issuer: required("OIDC_ISSUER")?,
                client_id: required("OIDC_CLIENT_ID")?,
                client_secret: secret("OIDC_CLIENT_SECRET")?,
                redirect_uri: required("OIDC_REDIRECT_URI")?,
                scopes: required("OIDC_SCOPES")?
                    .split_ascii_whitespace()
                    .map(str::to_owned)
                    .collect(),
            },
            oauth: OAuthConfig {
                issuer: required("OAUTH_ISSUER")?,
                resource: required("OAUTH_RESOURCE")?,
                required_scopes: parse_required_scopes(&required("OAUTH_REQUIRED_SCOPES")?)?,
                access_token_ttl: seconds("OAUTH_ACCESS_TOKEN_TTL")?,
                refresh_token_ttl: seconds("OAUTH_REFRESH_TOKEN_TTL")?,
                refresh_family_ttl: seconds("OAUTH_REFRESH_FAMILY_TTL")?,
                code_ttl: seconds("OAUTH_CODE_TTL")?,
                allow_dcr: boolean("OAUTH_ALLOW_DCR")?,
                allow_cimd: boolean("OAUTH_ALLOW_CIMD")?,
                cimd_trusted_private_origins: secure_origins("OAUTH_CIMD_TRUSTED_PRIVATE_ORIGINS")?,
                allow_loopback_redirects: boolean("OAUTH_ALLOW_LOOPBACK_REDIRECTS")?,
                wrapping_keys_file: required("OAUTH_WRAPPING_KEYS_FILE")?,
            },
            integrations: IntegrationsConfig {
                grafana: GrafanaConfig {
                    origin: secure_origin("GRAFANA_URL", &required("GRAFANA_URL")?)?,
                    token: secret("GRAFANA_TOKEN")?,
                },
                tekton: TektonConfig {
                    forgejo_origin: secure_origin("FORGEJO_ORIGIN", &required("FORGEJO_ORIGIN")?)?,
                    forgejo_token: secret("FORGEJO_TOKEN")?,
                    namespace: required("TEKTON_NAMESPACE")?,
                    pac_origin: internal_http_origin("PAC_URL", &required("PAC_URL")?)?,
                    pac_incoming_secret: secret("PAC_INCOMING_SECRET")?,
                },
                kubernetes: KubernetesIntegrationConfig {
                    kubectl_path: required("KUBECTL_PATH")?,
                    clusters: parse_kubernetes_clusters(&required("KUBERNETES_CLUSTERS")?)?,
                },
            },
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        let public = secure_url("PUBLIC_URL", &self.oidc.public_url)?;
        let oidc_issuer = secure_url("OIDC_ISSUER", &self.oidc.issuer)?;
        let redirect = secure_url("OIDC_REDIRECT_URI", &self.oidc.redirect_uri)?;
        let oauth_issuer = secure_url("OAUTH_ISSUER", &self.oauth.issuer)?;
        let resource = secure_url("OAUTH_RESOURCE", &self.oauth.resource)?;

        if oidc_issuer.query().is_some()
            || oidc_issuer.fragment().is_some()
            || public.path() != "/"
            || public.query().is_some()
            || public.fragment().is_some()
            || oauth_issuer.as_str() != format!("{}oauth", public.as_str())
            || resource.as_str() != format!("{}mcp", public.as_str())
            || redirect.as_str() != format!("{}oidc/callback", public.as_str())
        {
            return Err("public OAuth URLs are inconsistent".to_owned());
        }
        if ["openid", "profile", "email"]
            .iter()
            .any(|required| !self.oidc.scopes.iter().any(|scope| scope == required))
            || self.oauth.required_scopes != REQUIRED_OAUTH_SCOPES.map(str::to_owned)
            || self.oauth.access_token_ttl.is_zero()
            || self.oauth.refresh_token_ttl.is_zero()
            || self.oauth.refresh_family_ttl < self.oauth.refresh_token_ttl
            || self.oauth.code_ttl.is_zero()
        {
            return Err("OAuth policy configuration is invalid".to_owned());
        }
        if self.integrations.tekton.namespace != "pipelines-as-code"
            || self.integrations.tekton.forgejo_origin.as_str() != "https://git.holdenitdown.net/"
            || self.integrations.tekton.pac_origin.as_str()
                != "http://pipelines-as-code-controller.pipelines-as-code.svc.cluster.local:8080/"
        {
            return Err("Tekton integration configuration is invalid".to_owned());
        }
        validate_kubernetes_config(&self.integrations.kubernetes)?;
        Ok(())
    }
}

impl OAuthConfig {
    pub fn load_keyring(&self) -> Result<Arc<VersionedOAuthWrappingKeyring>, String> {
        let bytes = fs::read(&self.wrapping_keys_file)
            .map_err(|_| "failed to read OAuth wrapping keyring".to_owned())?;
        let file: KeyringFile = serde_json::from_slice(&bytes)
            .map_err(|_| "invalid OAuth wrapping keyring".to_owned())?;
        if file.schema_version != 1 || file.keys.is_empty() {
            return Err("unsupported OAuth wrapping keyring".to_owned());
        }
        let keys = file
            .keys
            .into_iter()
            .map(|entry| {
                URL_SAFE_NO_PAD
                    .decode(entry.key)
                    .map(|key| (entry.id, key))
                    .map_err(|_| "invalid OAuth wrapping keyring".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        VersionedOAuthWrappingKeyring::new(file.active, keys)
            .map(Arc::new)
            .map_err(|_| "invalid OAuth wrapping keyring".to_owned())
    }
}

fn required(name: &str) -> Result<String, String> {
    env::var(format!("{PREFIX}{name}"))
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing {PREFIX}{name}"))
}

fn optional(name: &str) -> Option<String> {
    env::var(format!("{PREFIX}{name}"))
        .ok()
        .filter(|value| !value.is_empty())
}

fn secret(name: &str) -> Result<Secret, String> {
    let value = required(name)?;
    if value.trim().is_empty() {
        return Err(format!("invalid {PREFIX}{name}"));
    }
    Ok(Secret(value))
}

fn seconds(name: &str) -> Result<Duration, String> {
    required(name)?
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("invalid {PREFIX}{name}"))
}

fn boolean(name: &str) -> Result<bool, String> {
    match required(name)?.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("invalid {PREFIX}{name}")),
    }
}

fn secure_url(name: &str, value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| format!("invalid {PREFIX}{name}"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(format!("invalid {PREFIX}{name}"));
    }
    Ok(url)
}

fn secure_origin(name: &str, value: &str) -> Result<Url, String> {
    let url = secure_url(name, value)?;
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(format!("invalid {PREFIX}{name}"));
    }
    Ok(url)
}

fn internal_http_origin(name: &str, value: &str) -> Result<Url, String> {
    let mut url = Url::parse(value).map_err(|_| format!("invalid {PREFIX}{name}"))?;
    if url.scheme() != "http"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || (url.path() != "/" && !url.path().is_empty())
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!("invalid {PREFIX}{name}"));
    }
    url.set_path("/");
    Ok(url)
}

fn secure_origins(name: &str) -> Result<Vec<Url>, String> {
    optional(name)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .map(|value| secure_origin(name, value))
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn parse_required_scopes(value: &str) -> Result<Vec<String>, String> {
    let scopes = value
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let unique = scopes.iter().collect::<HashSet<_>>();
    if scopes != REQUIRED_OAUTH_SCOPES.map(str::to_owned) || unique.len() != scopes.len() {
        return Err("OAuth policy configuration is invalid".to_owned());
    }
    Ok(scopes)
}

fn parse_kubernetes_clusters(value: &str) -> Result<Vec<KubernetesClusterConfig>, String> {
    if value.len() > MAX_KUBERNETES_CLUSTERS_JSON_BYTES {
        return Err("invalid HOMELAB_MCP_KUBERNETES_CLUSTERS".to_owned());
    }
    let clusters = serde_json::from_str(value)
        .map_err(|_| "invalid HOMELAB_MCP_KUBERNETES_CLUSTERS".to_owned())?;
    let config = KubernetesIntegrationConfig {
        kubectl_path: "/inert/kubectl".to_owned(),
        clusters,
    };
    validate_kubernetes_config(&config)?;
    Ok(config.clusters)
}

fn validate_kubernetes_config(config: &KubernetesIntegrationConfig) -> Result<(), String> {
    if !safe_absolute_path(&config.kubectl_path)
        || config.clusters.is_empty()
        || config.clusters.len() > 32
    {
        return Err("Kubernetes integration configuration is invalid".to_owned());
    }
    let mut names = HashSet::new();
    for cluster in &config.clusters {
        if !valid_kubernetes_identifier(&cluster.name, false)
            || !valid_kubernetes_identifier(&cluster.context, true)
            || !safe_absolute_path(&cluster.kubeconfig)
            || !safe_absolute_path(&cluster.cache_dir)
            || !names.insert(&cluster.name)
        {
            return Err("Kubernetes integration configuration is invalid".to_owned());
        }
    }
    Ok(())
}

fn safe_absolute_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.chars().any(char::is_control)
        && Path::new(value).is_absolute()
        && !Path::new(value)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

fn valid_kubernetes_identifier(value: &str, context: bool) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value.starts_with('-')
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '-' | '.' | '_')
                || context && matches!(c, ':' | '/')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_config_preserves_approved_metadata() {
        let config = TelemetryConfig {
            deployment_environment: "preview".to_owned(),
            k8s_namespace: Some("observability".to_owned()),
            k8s_pod_name: Some("homelab-mcp-abc".to_owned()),
            k8s_pod_uid: Some("pod-uid".to_owned()),
            pyroscope_url: None,
        };

        assert_eq!(config.deployment_environment, "preview");
        assert_eq!(config.k8s_namespace.as_deref(), Some("observability"));
        assert_eq!(config.k8s_pod_name.as_deref(), Some("homelab-mcp-abc"));
        assert_eq!(config.k8s_pod_uid.as_deref(), Some("pod-uid"));
        assert!(config.pyroscope_url.is_none());
    }

    #[test]
    fn pyroscope_url_requires_a_credential_free_https_origin() {
        for value in [
            "https://pyroscope.example/",
            "https://pyroscope.example:4040/",
        ] {
            assert!(secure_origin("PYROSCOPE_URL", value).is_ok());
        }
        for value in [
            "http://pyroscope.example/",
            "https://user@pyroscope.example/",
            "https://user:password@pyroscope.example/",
            "https://pyroscope.example/path",
            "https://pyroscope.example/?tenant=secret",
            "https://pyroscope.example/#fragment",
        ] {
            assert_eq!(
                secure_origin("PYROSCOPE_URL", value).unwrap_err(),
                "invalid HOMELAB_MCP_PYROSCOPE_URL"
            );
        }
    }

    #[test]
    fn keyring_parser_accepts_exact_format() {
        let file: KeyringFile = serde_json::from_str(
            r#"{"schema_version":1,"active":"v1","keys":[{"id":"v1","key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}]}"#,
        )
        .unwrap();
        let keys = file
            .keys
            .into_iter()
            .map(|entry| (entry.id, URL_SAFE_NO_PAD.decode(entry.key).unwrap()))
            .collect::<Vec<_>>();

        let keyring = VersionedOAuthWrappingKeyring::new(file.active, keys).unwrap();
        assert_eq!(keyring.current_key_id(), "v1");
    }

    #[test]
    fn keyring_parser_rejects_unknown_fields() {
        assert!(
            serde_json::from_str::<KeyringFile>(
                r#"{"schema_version":1,"active":"v1","keys":[],"extra":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn grafana_origin_requires_credential_free_https_root() {
        assert!(secure_origin("GRAFANA_URL", "https://grafana.example/").is_ok());
        for value in [
            "http://grafana.example/",
            "https://user@grafana.example/",
            "https://grafana.example/path",
            "https://grafana.example/?token=secret",
        ] {
            assert_eq!(
                secure_origin("GRAFANA_URL", value).unwrap_err(),
                "invalid HOMELAB_MCP_GRAFANA_URL"
            );
        }
    }

    #[test]
    fn trusted_cimd_origins_require_exact_https_origins() {
        for value in ["https://kuri.example/", "https://kuri.example:8443/"] {
            assert!(secure_origin("OAUTH_CIMD_TRUSTED_PRIVATE_ORIGINS", value).is_ok());
        }
        for value in [
            "http://kuri.example/",
            "https://user@kuri.example/",
            "https://kuri.example/client",
            "https://kuri.example/?tenant=private",
        ] {
            assert_eq!(
                secure_origin("OAUTH_CIMD_TRUSTED_PRIVATE_ORIGINS", value).unwrap_err(),
                "invalid HOMELAB_MCP_OAUTH_CIMD_TRUSTED_PRIVATE_ORIGINS"
            );
        }
    }

    #[test]
    fn config_requires_browser_claim_scopes_and_mcp_use() {
        let mut config = Config {
            database: DatabaseConfig {
                url: "postgres://localhost/test".to_owned(),
            },
            oidc: OidcConfig {
                public_url: "https://mcp.example/".to_owned(),
                issuer: "https://auth.example/application/o/homelab/".to_owned(),
                client_id: "client".to_owned(),
                client_secret: Secret("secret".to_owned()),
                redirect_uri: "https://mcp.example/oidc/callback".to_owned(),
                scopes: vec![
                    "openid".to_owned(),
                    "profile".to_owned(),
                    "email".to_owned(),
                ],
            },
            oauth: OAuthConfig {
                issuer: "https://mcp.example/oauth".to_owned(),
                resource: "https://mcp.example/mcp".to_owned(),
                required_scopes: vec!["other:scope".to_owned()],
                access_token_ttl: Duration::from_secs(60),
                refresh_token_ttl: Duration::from_secs(60),
                refresh_family_ttl: Duration::from_secs(120),
                code_ttl: Duration::from_secs(60),
                allow_dcr: false,
                allow_cimd: false,
                cimd_trusted_private_origins: Vec::new(),
                allow_loopback_redirects: false,
                wrapping_keys_file: "/unused".to_owned(),
            },
            integrations: IntegrationsConfig {
                grafana: GrafanaConfig {
                    origin: Url::parse("https://grafana.example/").unwrap(),
                    token: Secret("secret".to_owned()),
                },
                tekton: TektonConfig {
                    forgejo_origin: Url::parse("https://git.holdenitdown.net/").unwrap(),
                    forgejo_token: Secret("forgejo-secret".to_owned()),
                    namespace: "pipelines-as-code".to_owned(),
                    pac_origin: Url::parse("http://pipelines-as-code-controller.pipelines-as-code.svc.cluster.local:8080/").unwrap(),
                    pac_incoming_secret: Secret("pac-secret".to_owned()),
                },
                kubernetes: KubernetesIntegrationConfig {
                    kubectl_path: "/inert/kubectl".to_owned(),
                    clusters: vec![KubernetesClusterConfig {
                        name: "test".to_owned(),
                        kubeconfig: "/inert/kubeconfig".to_owned(),
                        context: "test-context".to_owned(),
                        cache_dir: "/inert/cache".to_owned(),
                    }],
                },
            },
        };

        assert_eq!(
            config.validate().unwrap_err(),
            "OAuth policy configuration is invalid"
        );
        config.oauth.required_scopes = REQUIRED_OAUTH_SCOPES.map(str::to_owned).to_vec();
        assert!(config.validate().is_ok());
        for required in ["openid", "profile", "email"] {
            let mut missing_scope = config.clone();
            missing_scope.oidc.scopes.retain(|scope| scope != required);
            assert_eq!(
                missing_scope.validate().unwrap_err(),
                "OAuth policy configuration is invalid"
            );
        }
    }

    #[test]
    fn required_oauth_scopes_are_exact_ordered_and_unique() {
        let canonical = "mcp:use kubernetes:read kubernetes:write";
        assert_eq!(
            parse_required_scopes(canonical).unwrap(),
            REQUIRED_OAUTH_SCOPES.map(str::to_owned)
        );
        for invalid in [
            "",
            "mcp:use",
            "kubernetes:read mcp:use kubernetes:write",
            "mcp:use kubernetes:read kubernetes:read",
            "mcp:use kubernetes:read admin",
        ] {
            assert!(
                parse_required_scopes(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn kubernetes_cluster_json_is_bounded_exact_and_path_only() {
        let valid = r#"[{"name":"pantheon","kubeconfig":"/run/kubernetes/pantheon","context":"mcp/pantheon","cache_dir":"/var/cache/kubectl/pantheon"}]"#;
        assert_eq!(
            parse_kubernetes_clusters(valid).unwrap()[0].name,
            "pantheon"
        );
        for invalid in [
            "[]",
            "not-json",
            r#"[{"name":"pantheon","kubeconfig":"relative","context":"ctx","cache_dir":"/cache"}]"#,
            r#"[{"name":"-bad","kubeconfig":"/k","context":"ctx","cache_dir":"/cache"}]"#,
            r#"[{"name":"pantheon","kubeconfig":"/k","context":"bad context","cache_dir":"/cache"}]"#,
            r#"[{"name":"pantheon","kubeconfig":"/k","context":"ctx","cache_dir":"/cache","token":"secret"}]"#,
            r#"[{"name":"same","kubeconfig":"/a","context":"a","cache_dir":"/a"},{"name":"same","kubeconfig":"/b","context":"b","cache_dir":"/b"}]"#,
        ] {
            assert!(
                parse_kubernetes_clusters(invalid).is_err(),
                "accepted {invalid}"
            );
        }
        assert!(
            parse_kubernetes_clusters(&" ".repeat(MAX_KUBERNETES_CLUSTERS_JSON_BYTES + 1)).is_err()
        );
    }
}
