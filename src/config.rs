use std::{env, fs, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use mcp::VersionedOAuthWrappingKeyring;
use serde::Deserialize;
use url::Url;

const PREFIX: &str = "HOMELAB_MCP_";

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub public_url: String,
    pub oidc_issuer: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,
    pub oidc_redirect_uri: String,
    pub oidc_scopes: Vec<String>,
    pub oauth_issuer: String,
    pub oauth_resource: String,
    pub oauth_required_scope: String,
    pub access_token_ttl: Duration,
    pub refresh_token_ttl: Duration,
    pub refresh_family_ttl: Duration,
    pub code_ttl: Duration,
    pub allow_dcr: bool,
    pub allow_cimd: bool,
    pub allow_loopback_redirects: bool,
    pub wrapping_keys_file: String,
    pub grafana_url: Url,
    pub grafana_token: Secret,
}

#[derive(Clone)]
pub struct Secret(pub(crate) String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
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
            database_url: required("DATABASE_URL")?,
            public_url: required("PUBLIC_URL")?,
            oidc_issuer: required("OIDC_ISSUER")?,
            oidc_client_id: required("OIDC_CLIENT_ID")?,
            oidc_client_secret: required("OIDC_CLIENT_SECRET")?,
            oidc_redirect_uri: required("OIDC_REDIRECT_URI")?,
            oidc_scopes: required("OIDC_SCOPES")?
                .split_ascii_whitespace()
                .map(str::to_owned)
                .collect(),
            oauth_issuer: required("OAUTH_ISSUER")?,
            oauth_resource: required("OAUTH_RESOURCE")?,
            oauth_required_scope: required("OAUTH_REQUIRED_SCOPE")?,
            access_token_ttl: seconds("OAUTH_ACCESS_TOKEN_TTL")?,
            refresh_token_ttl: seconds("OAUTH_REFRESH_TOKEN_TTL")?,
            refresh_family_ttl: seconds("OAUTH_REFRESH_FAMILY_TTL")?,
            code_ttl: seconds("OAUTH_CODE_TTL")?,
            allow_dcr: boolean("OAUTH_ALLOW_DCR")?,
            allow_cimd: boolean("OAUTH_ALLOW_CIMD")?,
            allow_loopback_redirects: boolean("OAUTH_ALLOW_LOOPBACK_REDIRECTS")?,
            wrapping_keys_file: required("OAUTH_WRAPPING_KEYS_FILE")?,
            grafana_url: secure_origin("GRAFANA_URL", &required("GRAFANA_URL")?)?,
            grafana_token: secret("GRAFANA_TOKEN")?,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        let public = secure_url("PUBLIC_URL", &self.public_url)?;
        let oidc_issuer = secure_url("OIDC_ISSUER", &self.oidc_issuer)?;
        let redirect = secure_url("OIDC_REDIRECT_URI", &self.oidc_redirect_uri)?;
        let oauth_issuer = secure_url("OAUTH_ISSUER", &self.oauth_issuer)?;
        let resource = secure_url("OAUTH_RESOURCE", &self.oauth_resource)?;

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
        if self.oidc_scopes.is_empty()
            || !self.oidc_scopes.iter().any(|scope| scope == "openid")
            || self.oauth_required_scope != "mcp:use"
            || self.access_token_ttl.is_zero()
            || self.refresh_token_ttl.is_zero()
            || self.refresh_family_ttl < self.refresh_token_ttl
            || self.code_ttl.is_zero()
        {
            return Err("OAuth policy configuration is invalid".to_owned());
        }
        Ok(())
    }

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn config_rejects_any_required_scope_other_than_mcp_use() {
        let mut config = Config {
            database_url: "postgres://localhost/test".to_owned(),
            public_url: "https://mcp.example/".to_owned(),
            oidc_issuer: "https://auth.example/application/o/homelab/".to_owned(),
            oidc_client_id: "client".to_owned(),
            oidc_client_secret: "secret".to_owned(),
            oidc_redirect_uri: "https://mcp.example/oidc/callback".to_owned(),
            oidc_scopes: vec!["openid".to_owned()],
            oauth_issuer: "https://mcp.example/oauth".to_owned(),
            oauth_resource: "https://mcp.example/mcp".to_owned(),
            oauth_required_scope: "other:scope".to_owned(),
            access_token_ttl: Duration::from_secs(60),
            refresh_token_ttl: Duration::from_secs(60),
            refresh_family_ttl: Duration::from_secs(120),
            code_ttl: Duration::from_secs(60),
            allow_dcr: false,
            allow_cimd: false,
            allow_loopback_redirects: false,
            wrapping_keys_file: "/unused".to_owned(),
            grafana_url: Url::parse("https://grafana.example/").unwrap(),
            grafana_token: Secret("secret".to_owned()),
        };

        assert_eq!(
            config.validate().unwrap_err(),
            "OAuth policy configuration is invalid"
        );
        config.oauth_required_scope = "mcp:use".to_owned();
        assert!(config.validate().is_ok());
    }
}
