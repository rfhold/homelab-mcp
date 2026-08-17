use std::{future::Future, path::PathBuf, pin::Pin, time::Duration};

use reqwest::{Client, Url, redirect::Policy};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;
use zeroize::Zeroizing;

use super::DeployError;

const MAX_JWT_BYTES: u64 = 16 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
struct LoginResponse {
    auth: LoginAuth,
}

#[derive(Deserialize)]
struct LoginAuth {
    client_token: String,
}

#[derive(Deserialize)]
struct SignResponse {
    data: SignData,
}

#[derive(Deserialize)]
struct SignData {
    signed_key: String,
}

pub trait OpenBaoSigner: Send + Sync + 'static {
    fn sign<'a>(
        &'a self,
        public_key: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, DeployError>> + Send + 'a>>;
}

#[derive(Clone, Debug)]
pub struct OpenBaoConfig {
    pub origin: Url,
    pub kubernetes_auth_mount: String,
    pub kubernetes_role: String,
    pub ssh_mount: String,
    pub ssh_role: String,
    pub jwt_path: PathBuf,
    pub request_timeout: Duration,
}

#[derive(Clone)]
pub struct OpenBaoClient {
    config: OpenBaoConfig,
    http: Client,
}

impl OpenBaoClient {
    pub fn new(config: OpenBaoConfig) -> Result<Self, DeployError> {
        if config.origin.scheme() != "https"
            || config.origin.cannot_be_a_base()
            || config.origin.path() != "/"
            || config.origin.query().is_some()
            || config.origin.fragment().is_some()
            || !config.jwt_path.is_absolute()
            || !valid_segment(&config.kubernetes_auth_mount)
            || !valid_segment(&config.kubernetes_role)
            || !valid_segment(&config.ssh_mount)
            || !valid_segment(&config.ssh_role)
            || !(Duration::from_millis(100)..=Duration::from_secs(30))
                .contains(&config.request_timeout)
        {
            return Err(DeployError::InvalidConfiguration);
        }
        let http = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .timeout(config.request_timeout)
            .build()
            .map_err(|_| DeployError::InvalidConfiguration)?;
        Ok(Self { config, http })
    }

    #[cfg(test)]
    fn new_for_test(config: OpenBaoConfig) -> Result<Self, DeployError> {
        let http = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .timeout(config.request_timeout)
            .build()
            .map_err(|_| DeployError::InvalidConfiguration)?;
        Ok(Self { config, http })
    }

    async fn request_certificate(&self, public_key: &str) -> Result<String, DeployError> {
        validate_public_key(public_key)?;
        let metadata = std::fs::metadata(&self.config.jwt_path)
            .map_err(|_| DeployError::CredentialUnavailable)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_JWT_BYTES {
            return Err(DeployError::CredentialUnavailable);
        }
        let jwt = Zeroizing::new(
            std::fs::read_to_string(&self.config.jwt_path)
                .map_err(|_| DeployError::CredentialUnavailable)?,
        );
        if jwt.trim() != jwt.as_str() || jwt.chars().any(char::is_whitespace) {
            return Err(DeployError::CredentialInvalid);
        }
        let login_url = self.endpoint(&format!(
            "v1/auth/{}/login",
            self.config.kubernetes_auth_mount
        ))?;
        let response = self
            .http
            .post(login_url)
            .json(&json!({"role": self.config.kubernetes_role, "jwt": jwt.as_str()}))
            .send()
            .await
            .map_err(|_| DeployError::CredentialUnavailable)?;
        let login: LoginResponse = bounded_json(response).await?;
        let token = Zeroizing::new(login.auth.client_token);
        if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_whitespace) {
            return Err(DeployError::CredentialInvalid);
        }
        let sign_url = self.endpoint(&format!(
            "v1/{}/sign/{}",
            self.config.ssh_mount, self.config.ssh_role
        ))?;
        let response = self.http.post(sign_url).header("X-Vault-Token", token.as_str()).json(&json!({
            "public_key": public_key, "cert_type": "user", "valid_principals": "homelab", "ttl": "15m"
        })).send().await.map_err(|_| DeployError::CredentialUnavailable)?;
        let signed: SignResponse = bounded_json(response).await?;
        let signed = signed.data.signed_key;
        validate_certificate(&signed)?;
        Ok(signed)
    }

    fn endpoint(&self, path: &str) -> Result<Url, DeployError> {
        self.config
            .origin
            .join(path)
            .map_err(|_| DeployError::InvalidConfiguration)
    }
}

impl OpenBaoSigner for OpenBaoClient {
    fn sign<'a>(
        &'a self,
        public_key: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, DeployError>> + Send + 'a>> {
        Box::pin(self.request_certificate(public_key))
    }
}

async fn bounded_json<T: DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, DeployError> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(DeployError::CredentialUnavailable);
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| DeployError::CredentialUnavailable)?
    {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
            return Err(DeployError::CredentialInvalid);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| DeployError::CredentialInvalid)
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn validate_public_key(value: &str) -> Result<(), DeployError> {
    let mut fields = value.split(' ');
    if fields.next() != Some("ssh-ed25519")
        || fields.next().is_none_or(str::is_empty)
        || fields.next().is_some()
        || value.len() > 1024
    {
        return Err(DeployError::CredentialInvalid);
    }
    Ok(())
}

fn validate_certificate(value: &str) -> Result<(), DeployError> {
    let mut fields = value.trim_end_matches('\n').split(' ');
    if fields.next() != Some("ssh-ed25519-cert-v01@openssh.com")
        || fields.next().is_none_or(str::is_empty)
        || fields.next().is_some()
        || value.len() > 16 * 1024
        || value.contains('\r')
        || value.trim_end_matches('\n').contains('\n')
    {
        return Err(DeployError::CredentialInvalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        sync::{Arc, Mutex},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[tokio::test]
    async fn sends_exact_bounded_requests_and_redacts_failures() {
        let dir = std::env::temp_dir().join(format!(
            "homelab-openbao-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let jwt_path = dir.join("jwt");
        fs::write(&jwt_path, "projected-jwt-secret").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let server = tokio::spawn(async move {
            for response in [
                r#"{"auth":{"client_token":"short-lived-token-secret"}}"#,
                r#"{"data":{"signed_key":"ssh-ed25519-cert-v01@openssh.com AAAATEST"}}"#,
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let read = stream.read(&mut buffer).await.unwrap();
                    bytes.extend_from_slice(&buffer[..read]);
                    let text = String::from_utf8_lossy(&bytes);
                    let Some(header_end) = text.find("\r\n\r\n") else {
                        continue;
                    };
                    let length = text[..header_end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= header_end + 4 + length {
                        break;
                    }
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).as_bytes()).await.unwrap();
            }
        });
        let client = OpenBaoClient::new_for_test(OpenBaoConfig {
            origin: Url::parse(&format!("http://{address}/")).unwrap(),
            kubernetes_auth_mount: "kubernetes".into(),
            kubernetes_role: "homelab-mcp".into(),
            ssh_mount: "ssh-client-signer".into(),
            ssh_role: "homelab".into(),
            jwt_path,
            request_timeout: Duration::from_secs(2),
        })
        .unwrap();
        let certificate = client.sign("ssh-ed25519 AAAATEST").await.unwrap();
        assert_eq!(certificate, "ssh-ed25519-cert-v01@openssh.com AAAATEST");
        server.await.unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("POST /v1/auth/kubernetes/login HTTP/1.1"));
        let login: Value =
            serde_json::from_str(requests[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            login,
            json!({"role":"homelab-mcp","jwt":"projected-jwt-secret"})
        );
        assert!(!requests[0].to_ascii_lowercase().contains("x-vault-token"));
        assert!(requests[1].starts_with("POST /v1/ssh-client-signer/sign/homelab HTTP/1.1"));
        assert!(requests[1].contains("x-vault-token: short-lived-token-secret"));
        let sign: Value =
            serde_json::from_str(requests[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            sign,
            json!({"cert_type":"user","public_key":"ssh-ed25519 AAAATEST","ttl":"15m","valid_principals":"homelab"})
        );
        for error in [
            DeployError::CredentialUnavailable,
            DeployError::CredentialInvalid,
        ] {
            let rendered = format!("{error:?} {error}");
            assert!(!rendered.contains("projected-jwt-secret"));
            assert!(!rendered.contains("short-lived-token-secret"));
            assert!(!rendered.contains("AAAATEST"));
        }
        drop(requests);
        fs::remove_dir_all(dir).unwrap();
    }
}
