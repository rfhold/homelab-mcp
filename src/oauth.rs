use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::response::IntoResponse as _;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use mcp::{
    HardenedOAuthClientMetadataFetcher, McpOAuthEntropy, McpOAuthSecret, McpPrincipalId,
    McpSystemOAuthClock, OAuthAuthorizationServer, OAuthAuthorizationServerConfig,
    OAuthAuthorizationStore as _, OAuthClientRegistrationOptions, OAuthConsentDecisionEvidence,
    OAuthConsentHandler, OAuthConsentModel, OAuthConsentPresentation, OAuthResource,
    OAuthSigningKeyState, OidcEndpointPolicy, OidcPrincipalMapper, OidcPrincipalMapping,
    OidcResourceOwnerAuthenticator, OidcResourceOwnerConfig, OidcVerifiedIdentity,
    PostgresOAuthAuthorizationStore, PostgresOidcResourceOwnerStore,
    TrustedPrivateOAuthCimdDestinationPolicy, server::BoxFuture,
};
use sha2::{Digest as _, Sha256};
use sqlx::PgPool;

use crate::{
    app::ReadinessCheck,
    config::{OAuthConfig, OidcConfig},
};

const READINESS_TIMEOUT: Duration = Duration::from_secs(2);

pub struct OAuthRuntime {
    pub server: OAuthAuthorizationServer,
    pub oidc: OidcResourceOwnerAuthenticator,
    pool: PgPool,
}

pub async fn initialize(
    pool: PgPool,
    oidc_config: &OidcConfig,
    oauth: &OAuthConfig,
) -> Result<OAuthRuntime, String> {
    let store = Arc::new(
        PostgresOAuthAuthorizationStore::from_pool(pool.clone())
            .await
            .map_err(|_| "failed to initialize OAuth persistence".to_owned())?,
    );
    let oidc_store = Arc::new(
        PostgresOidcResourceOwnerStore::from_pool(pool.clone())
            .await
            .map_err(|_| "failed to initialize OIDC persistence".to_owned())?,
    );
    let keyring = oauth.load_keyring()?;
    let entropy: Arc<dyn McpOAuthEntropy> = Arc::new(SystemEntropy);
    let oidc_config = OidcResourceOwnerConfig::new(
        oidc_config.issuer.clone(),
        oidc_config.client_id.clone(),
        Some(McpOAuthSecret::new(
            oidc_config.client_secret.expose().to_owned(),
        )),
        &oidc_config.redirect_uri,
        format!("{}/authorize", oauth.issuer.trim_end_matches('/')),
        oidc_config.scopes.clone(),
        oauth.code_ttl.min(Duration::from_secs(10 * 60)),
        OidcEndpointPolicy::HttpsOnly,
    )
    .map_err(|_| "invalid OIDC resource-owner configuration".to_owned())?;
    let oidc = OidcResourceOwnerAuthenticator::discover(
        oidc_config,
        oidc_store,
        Arc::new(StableOidcPrincipalMapper),
        Arc::new(McpSystemOAuthClock),
        entropy.clone(),
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(10)),
    )
    .await
    .map_err(|_| "failed to initialize OIDC resource-owner authentication".to_owned())?;
    let policy = authorization_server_policy(oauth)?;

    let consent = Arc::new(AutoApproveConsent {
        resource: oauth.resource.clone(),
        scopes: oauth.required_scopes.clone(),
    });
    let mut server = OAuthAuthorizationServer::new(
        policy,
        store.clone(),
        Arc::new(oidc.clone()),
        consent,
        Arc::new(McpSystemOAuthClock),
        entropy,
        keyring,
    )
    .map_err(|_| "invalid hosted OAuth configuration".to_owned())?;

    if oauth.allow_dcr || oauth.allow_cimd || oauth.allow_loopback_redirects {
        let metadata_fetcher = if oauth.allow_cimd {
            let mut fetcher = HardenedOAuthClientMetadataFetcher::production()
                .with_loopback_redirects(oauth.allow_loopback_redirects);
            if !oauth.cimd_trusted_private_origins.is_empty() {
                let destination_policy = TrustedPrivateOAuthCimdDestinationPolicy::new(
                    oauth.cimd_trusted_private_origins.clone(),
                )
                .map_err(|_| "invalid trusted CIMD destination policy".to_owned())?;
                fetcher = fetcher.with_destination_policy(Arc::new(destination_policy));
            }
            Some(Arc::new(fetcher) as Arc<_>)
        } else {
            None
        };
        server = server
            .with_client_registration(OAuthClientRegistrationOptions {
                metadata_fetcher,
                dynamic_registration: oauth.allow_dcr,
                allow_loopback_redirects: oauth.allow_loopback_redirects,
                source_resolver: None,
                ..OAuthClientRegistrationOptions::default()
            })
            .map_err(|_| "invalid OAuth client registration policy".to_owned())?;
    }

    initialize_signing_key(&pool, store.as_ref(), &server, &oauth.issuer).await?;
    Ok(OAuthRuntime { server, oidc, pool })
}

fn authorization_server_policy(
    oauth: &OAuthConfig,
) -> Result<OAuthAuthorizationServerConfig, String> {
    let mut policy = OAuthAuthorizationServerConfig::new(
        oauth.issuer.clone(),
        vec![OAuthResource {
            resource: oauth.resource.clone(),
            scopes: oauth.required_scopes.clone(),
        }],
    );
    policy.authorization_code_lifetime = oauth.code_ttl;
    policy.access_token_lifetime = oauth.access_token_ttl;
    policy.refresh_token_lifetime = oauth.refresh_token_ttl;
    policy.refresh_family_lifetime = oauth.refresh_family_ttl;
    let required_verification_overlap = policy
        .access_token_lifetime
        .checked_add(policy.clock_skew)
        .ok_or_else(|| "invalid hosted OAuth configuration".to_owned())?;
    policy.signing_verification_overlap = policy
        .signing_verification_overlap
        .max(required_verification_overlap);
    if oauth.allow_dcr {
        policy.registration_endpoint =
            Some(format!("{}/register", oauth.issuer.trim_end_matches('/')));
    }
    Ok(policy)
}

async fn initialize_signing_key(
    pool: &PgPool,
    store: &PostgresOAuthAuthorizationStore,
    server: &OAuthAuthorizationServer,
    issuer: &str,
) -> Result<(), String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| "failed to lock OAuth signing-key initialization".to_owned())?;
    sqlx::query("SELECT pg_advisory_xact_lock(1213155660, 1331053396)")
        .execute(&mut *transaction)
        .await
        .map_err(|_| "failed to lock OAuth signing-key initialization".to_owned())?;

    let active = store
        .active_signing_key(issuer.to_owned())
        .await
        .map_err(|_| "failed to inspect OAuth signing-key state".to_owned())?;
    if active.is_none() {
        let generated = server
            .generate_signing_key_candidate(true)
            .await
            .map_err(|_| "failed to bootstrap OAuth signing key".to_owned())?;
        if generated.state != OAuthSigningKeyState::Active
            && !server
                .activate_signing_key(generated.key_id)
                .await
                .map_err(|_| "failed to activate OAuth signing key".to_owned())?
        {
            return Err("failed to activate OAuth signing key".to_owned());
        }
    }
    server
        .validate_signing_key_readiness()
        .await
        .map_err(|_| "OAuth signing key is not ready".to_owned())?;
    transaction
        .commit()
        .await
        .map_err(|_| "failed to complete OAuth signing-key initialization".to_owned())
}

impl ReadinessCheck for OAuthRuntime {
    fn check(&self) -> BoxFuture<bool> {
        let pool = self.pool.clone();
        let server = self.server.clone();
        Box::pin(async move {
            tokio::time::timeout(READINESS_TIMEOUT, async move {
                sqlx::query_scalar::<_, i32>("SELECT 1")
                    .fetch_one(&pool)
                    .await
                    .is_ok()
                    && server.validate_signing_key_readiness().await.is_ok()
            })
            .await
            .unwrap_or(false)
        })
    }
}

impl OAuthRuntime {
    pub async fn cleanup(&self, limit: usize) {
        let _ = self.server.cleanup(limit).await;
        let _ = self.oidc.cleanup(limit).await;
    }
}

struct StableOidcPrincipalMapper;

impl OidcPrincipalMapper for StableOidcPrincipalMapper {
    fn map(&self, identity: OidcVerifiedIdentity) -> BoxFuture<OidcPrincipalMapping> {
        Box::pin(async move {
            let mut principal = Sha256::new();
            principal.update(identity.issuer.as_bytes());
            principal.update([0]);
            principal.update(identity.subject.as_bytes());
            McpPrincipalId::new(format!(
                "authentik:{}",
                URL_SAFE_NO_PAD.encode(principal.finalize())
            ))
            .map_or(
                OidcPrincipalMapping::Denied,
                OidcPrincipalMapping::Principal,
            )
        })
    }
}

struct SystemEntropy;

impl McpOAuthEntropy for SystemEntropy {
    fn fill_bytes(&self, output: &mut [u8]) -> mcp::Result<()> {
        getrandom::fill(output).map_err(|_| mcp::Error::protocol("system entropy unavailable"))
    }
}

struct AutoApproveConsent {
    resource: String,
    scopes: Vec<String>,
}

impl OAuthConsentHandler for AutoApproveConsent {
    fn present(&self, model: OAuthConsentModel) -> BoxFuture<OAuthConsentPresentation> {
        let expected = self.scopes.iter().collect::<HashSet<_>>();
        let requested = model.requested_scopes.iter().collect::<HashSet<_>>();
        let approved = model.resource == self.resource
            && model.requested_scopes.len() == self.scopes.len()
            && requested == expected;
        Box::pin(async move {
            if approved {
                OAuthConsentPresentation::Approved
            } else {
                OAuthConsentPresentation::Response(
                    axum::http::StatusCode::FORBIDDEN.into_response(),
                )
            }
        })
    }

    fn validate_decision(&self, _: OAuthConsentDecisionEvidence) -> BoxFuture<bool> {
        Box::pin(async { false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oauth_config(access_token_ttl: Duration) -> OAuthConfig {
        OAuthConfig {
            issuer: "https://mcp.example".to_owned(),
            resource: "https://mcp.example/mcp".to_owned(),
            required_scopes: vec!["mcp:use".to_owned()],
            access_token_ttl,
            refresh_token_ttl: Duration::from_secs(24 * 60 * 60),
            refresh_family_ttl: Duration::from_secs(30 * 24 * 60 * 60),
            code_ttl: Duration::from_secs(5 * 60),
            allow_dcr: true,
            allow_cimd: false,
            cimd_trusted_private_origins: Vec::new(),
            allow_loopback_redirects: false,
            wrapping_keys_file: "unused-in-policy-test".to_owned(),
        }
    }

    #[test]
    fn policy_covers_multi_day_access_tokens_with_signing_overlap() {
        let oauth = oauth_config(Duration::from_secs(3 * 24 * 60 * 60));
        let policy = authorization_server_policy(&oauth).expect("policy should be valid");

        assert_eq!(
            policy.signing_verification_overlap,
            Duration::from_secs(259_230)
        );
        assert_eq!(policy.authorization_code_lifetime, oauth.code_ttl);
        assert_eq!(policy.access_token_lifetime, oauth.access_token_ttl);
        assert_eq!(policy.refresh_token_lifetime, oauth.refresh_token_ttl);
        assert_eq!(policy.refresh_family_lifetime, oauth.refresh_family_ttl);
        assert_eq!(
            policy.registration_endpoint.as_deref(),
            Some("https://mcp.example/register")
        );
    }

    #[test]
    fn policy_preserves_default_signing_overlap_for_short_access_tokens() {
        let policy = authorization_server_policy(&oauth_config(Duration::from_secs(5 * 60)))
            .expect("policy should be valid");

        assert_eq!(
            policy.signing_verification_overlap,
            Duration::from_secs(10 * 60)
        );
    }

    #[tokio::test]
    async fn consent_only_approves_the_exact_resource_and_scope_set() {
        let consent = AutoApproveConsent {
            resource: "https://mcp.example/mcp".to_owned(),
            scopes: [
                "mcp:use",
                "kubernetes:read",
                "kubernetes:write",
                "inventory:read",
                "inventory:write",
                "inventory:host-trust",
                "deploy:read",
                "deploy:run",
            ]
            .map(str::to_owned)
            .to_vec(),
        };
        let model = |resource: &str, scopes: &[&str]| OAuthConsentModel {
            client_id: "client".to_owned(),
            client_display_name: None,
            resource: resource.to_owned(),
            requested_scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
            already_granted_scopes: Vec::new(),
            transaction_id: "transaction".to_owned(),
        };

        assert!(matches!(
            consent
                .present(model(
                    "https://mcp.example/mcp",
                    &[
                        "deploy:run",
                        "inventory:write",
                        "kubernetes:write",
                        "mcp:use",
                        "deploy:read",
                        "inventory:host-trust",
                        "kubernetes:read",
                        "inventory:read",
                    ],
                ))
                .await,
            OAuthConsentPresentation::Approved
        ));
        assert!(matches!(
            consent
                .present(model(
                    "https://mcp.example/mcp",
                    &["mcp:use", "kubernetes:read", "admin"],
                ))
                .await,
            OAuthConsentPresentation::Response(_)
        ));
        assert!(matches!(
            consent
                .present(model(
                    "https://mcp.example/mcp",
                    &["mcp:use", "kubernetes:read"],
                ))
                .await,
            OAuthConsentPresentation::Response(_)
        ));
    }

    #[tokio::test]
    async fn principal_mapping_uses_only_verified_issuer_and_subject() {
        let identity = OidcVerifiedIdentity {
            issuer: "https://auth.example/application/o/homelab/".to_owned(),
            subject: "user-123".to_owned(),
            audiences: vec!["client".to_owned()],
            claims: serde_json::Map::from_iter([(
                "preferred_username".to_owned(),
                serde_json::json!("mutable-name"),
            )]),
        };
        let first = StableOidcPrincipalMapper.map(identity.clone()).await;
        let second = StableOidcPrincipalMapper
            .map(OidcVerifiedIdentity {
                claims: serde_json::Map::new(),
                ..identity
            })
            .await;
        assert_eq!(first, second);
        assert!(matches!(first, OidcPrincipalMapping::Principal(_)));
    }
}
