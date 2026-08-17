use std::{fmt, net::IpAddr, str::FromStr as _};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

const DEFAULT_SSH_PORT: u16 = 22;
const DEFAULT_SSH_USERNAME: &str = "homelab";
const MAX_LIST_LIMIT: u16 = 100;

type MachineRow = (
    Uuid,
    String,
    String,
    i32,
    String,
    Option<String>,
    DateTime<Utc>,
    DateTime<Utc>,
);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Machine {
    pub id: Uuid,
    pub display_name: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_username: String,
    pub pinned_host_public_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateMachine {
    pub display_name: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_username: String,
    pub pinned_host_public_key: Option<String>,
}

impl CreateMachine {
    pub fn new(display_name: impl Into<String>, ssh_host: impl Into<String>) -> Self {
        Self {
            display_name: display_name.into(),
            ssh_host: ssh_host.into(),
            ssh_port: DEFAULT_SSH_PORT,
            ssh_username: DEFAULT_SSH_USERNAME.to_owned(),
            pinned_host_public_key: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateMachine {
    pub display_name: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_username: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationError {
    DisplayName,
    SshHost,
    SshPort,
    SshUsername,
    HostPublicKey,
    ListLimit,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let field = match self {
            Self::DisplayName => "display name",
            Self::SshHost => "SSH host",
            Self::SshPort => "SSH port",
            Self::SshUsername => "SSH username",
            Self::HostPublicKey => "host public key",
            Self::ListLimit => "list limit",
        };
        write!(formatter, "invalid {field}")
    }
}

impl std::error::Error for ValidationError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryError {
    Validation(ValidationError),
    NotFound,
    Conflict,
    Database,
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::NotFound => formatter.write_str("machine not found"),
            Self::Conflict => formatter.write_str("machine conflicts with existing inventory"),
            Self::Database => formatter.write_str("machine inventory database operation failed"),
        }
    }
}

impl std::error::Error for RepositoryError {}

impl From<ValidationError> for RepositoryError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

#[derive(Clone)]
pub struct MachineRepository {
    pool: PgPool,
}

impl MachineRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, input: CreateMachine) -> Result<Machine, RepositoryError> {
        validate_machine_fields(
            &input.display_name,
            &input.ssh_host,
            input.ssh_port,
            &input.ssh_username,
        )?;
        if let Some(key) = input.pinned_host_public_key.as_deref() {
            validate_host_public_key(key)?;
        }

        sqlx::query_as::<_, MachineRow>(
            "INSERT INTO homelab.machines \
             (id, display_name, ssh_host, ssh_port, ssh_username, pinned_host_public_key) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING id, display_name, ssh_host, ssh_port, ssh_username, \
             pinned_host_public_key, created_at, updated_at",
        )
        .bind(Uuid::new_v4())
        .bind(input.display_name)
        .bind(input.ssh_host)
        .bind(i32::from(input.ssh_port))
        .bind(input.ssh_username)
        .bind(input.pinned_host_public_key)
        .fetch_one(&self.pool)
        .await
        .map(machine_from_row)
        .map_err(map_database_error)
    }

    pub async fn get(&self, id: Uuid) -> Result<Machine, RepositoryError> {
        sqlx::query_as::<_, MachineRow>(
            "SELECT id, display_name, ssh_host, ssh_port, ssh_username, \
             pinned_host_public_key, created_at, updated_at \
             FROM homelab.machines WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_database_error)?
        .map(machine_from_row)
        .ok_or(RepositoryError::NotFound)
    }

    pub async fn list(&self, limit: u16) -> Result<Vec<Machine>, RepositoryError> {
        validate_list_limit(limit)?;
        sqlx::query_as::<_, MachineRow>(
            "SELECT id, display_name, ssh_host, ssh_port, ssh_username, \
             pinned_host_public_key, created_at, updated_at \
             FROM homelab.machines ORDER BY display_name ASC, id ASC LIMIT $1",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(machine_from_row).collect())
        .map_err(map_database_error)
    }

    pub async fn update(&self, id: Uuid, input: UpdateMachine) -> Result<Machine, RepositoryError> {
        validate_machine_fields(
            &input.display_name,
            &input.ssh_host,
            input.ssh_port,
            &input.ssh_username,
        )?;
        sqlx::query_as::<_, MachineRow>(
            "UPDATE homelab.machines SET display_name = $2, ssh_host = $3, ssh_port = $4, \
             ssh_username = $5, updated_at = now() WHERE id = $1 \
             RETURNING id, display_name, ssh_host, ssh_port, ssh_username, \
             pinned_host_public_key, created_at, updated_at",
        )
        .bind(id)
        .bind(input.display_name)
        .bind(input.ssh_host)
        .bind(i32::from(input.ssh_port))
        .bind(input.ssh_username)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_database_error)?
        .map(machine_from_row)
        .ok_or(RepositoryError::NotFound)
    }

    pub async fn replace_host_key(
        &self,
        id: Uuid,
        host_public_key: String,
    ) -> Result<Machine, RepositoryError> {
        validate_host_public_key(&host_public_key)?;
        self.set_host_key(id, Some(host_public_key)).await
    }

    pub async fn clear_host_key(&self, id: Uuid) -> Result<Machine, RepositoryError> {
        self.set_host_key(id, None).await
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), RepositoryError> {
        let result = sqlx::query("DELETE FROM homelab.machines WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_database_error)?;
        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound);
        }
        Ok(())
    }

    async fn set_host_key(
        &self,
        id: Uuid,
        host_public_key: Option<String>,
    ) -> Result<Machine, RepositoryError> {
        sqlx::query_as::<_, MachineRow>(
            "UPDATE homelab.machines SET pinned_host_public_key = $2, updated_at = now() \
             WHERE id = $1 RETURNING id, display_name, ssh_host, ssh_port, ssh_username, \
             pinned_host_public_key, created_at, updated_at",
        )
        .bind(id)
        .bind(host_public_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_database_error)?
        .map(machine_from_row)
        .ok_or(RepositoryError::NotFound)
    }
}

fn machine_from_row(row: MachineRow) -> Machine {
    Machine {
        id: row.0,
        display_name: row.1,
        ssh_host: row.2,
        ssh_port: u16::try_from(row.3).expect("database SSH port constraint was violated"),
        ssh_username: row.4,
        pinned_host_public_key: row.5,
        created_at: row.6,
        updated_at: row.7,
    }
}

fn map_database_error(error: sqlx::Error) -> RepositoryError {
    if let sqlx::Error::Database(database) = &error
        && database.code().as_deref() == Some("23505")
    {
        return RepositoryError::Conflict;
    }
    RepositoryError::Database
}

fn validate_machine_fields(
    display_name: &str,
    ssh_host: &str,
    ssh_port: u16,
    ssh_username: &str,
) -> Result<(), ValidationError> {
    if display_name.is_empty()
        || display_name.len() > 100
        || display_name.trim() != display_name
        || !display_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || " ._-".contains(character))
    {
        return Err(ValidationError::DisplayName);
    }
    if !valid_ssh_host(ssh_host) {
        return Err(ValidationError::SshHost);
    }
    if ssh_port != DEFAULT_SSH_PORT {
        return Err(ValidationError::SshPort);
    }
    if ssh_username != DEFAULT_SSH_USERNAME {
        return Err(ValidationError::SshUsername);
    }
    Ok(())
}

fn validate_list_limit(limit: u16) -> Result<(), ValidationError> {
    if (1..=MAX_LIST_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(ValidationError::ListLimit)
    }
}

fn valid_ssh_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.trim() != host {
        return false;
    }
    if IpAddr::from_str(host).is_ok() {
        return true;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
    })
}

fn validate_host_public_key(key: &str) -> Result<(), ValidationError> {
    if key.len() > 256 || key.trim() != key {
        return Err(ValidationError::HostPublicKey);
    }
    let mut fields = key.split(' ');
    if fields.next() != Some("ssh-ed25519") {
        return Err(ValidationError::HostPublicKey);
    }
    let encoded = fields.next().ok_or(ValidationError::HostPublicKey)?;
    if encoded.is_empty() || fields.next().is_some() {
        return Err(ValidationError::HostPublicKey);
    }
    let decoded = STANDARD
        .decode(encoded)
        .map_err(|_| ValidationError::HostPublicKey)?;
    let algorithm = ssh_string(&decoded, 0).ok_or(ValidationError::HostPublicKey)?;
    let public_key = ssh_string(&decoded, algorithm.1).ok_or(ValidationError::HostPublicKey)?;
    if algorithm.0 != b"ssh-ed25519" || public_key.0.len() != 32 || public_key.1 != decoded.len() {
        return Err(ValidationError::HostPublicKey);
    }
    Ok(())
}

fn ssh_string(bytes: &[u8], offset: usize) -> Option<(&[u8], usize)> {
    let length_bytes: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    let length = usize::try_from(u32::from_be_bytes(length_bytes)).ok()?;
    let start = offset.checked_add(4)?;
    let end = start.checked_add(length)?;
    Some((bytes.get(start..end)?, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED25519_KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn defaults_are_safe_and_unpinned() {
        let machine = CreateMachine::new("node one", "node-1.example.test");
        assert_eq!(machine.ssh_port, 22);
        assert_eq!(machine.ssh_username, "homelab");
        assert_eq!(machine.pinned_host_public_key, None);
        assert!(
            validate_machine_fields(
                &machine.display_name,
                &machine.ssh_host,
                machine.ssh_port,
                &machine.ssh_username
            )
            .is_ok()
        );
    }

    #[test]
    fn machine_fields_accept_bounded_names_hosts_and_fixed_ssh_target() {
        for host in ["node-1.example.test", "192.0.2.1", "2001:db8::1"] {
            assert!(validate_machine_fields("Node 1", host, 22, "homelab").is_ok());
        }
    }

    #[test]
    fn machine_fields_reject_ambiguous_or_unbounded_values() {
        assert_eq!(
            validate_machine_fields(" node", "host", 22, "homelab"),
            Err(ValidationError::DisplayName)
        );
        assert_eq!(
            validate_machine_fields("node", "https://host", 22, "homelab"),
            Err(ValidationError::SshHost)
        );
        assert_eq!(
            validate_machine_fields("node", "host", 2222, "homelab"),
            Err(ValidationError::SshPort)
        );
        assert_eq!(
            validate_machine_fields("node", "host", 22, "root"),
            Err(ValidationError::SshUsername)
        );
    }

    #[test]
    fn only_exact_ed25519_public_keys_are_accepted() {
        assert_eq!(validate_host_public_key(ED25519_KEY), Ok(()));
        for invalid in [
            "command=x ssh-ed25519 AAAA",
            "ssh-rsa AAAA",
            "ssh-ed25519 AAAA comment",
            concat!("-----BEGIN OPENSSH ", "PRIVATE KEY-----"),
            "ssh-ed25519 not-base64",
        ] {
            assert_eq!(
                validate_host_public_key(invalid),
                Err(ValidationError::HostPublicKey)
            );
        }
    }

    #[test]
    fn list_limit_is_strictly_bounded() {
        assert_eq!(validate_list_limit(1), Ok(()));
        assert_eq!(validate_list_limit(100), Ok(()));
        assert_eq!(validate_list_limit(0), Err(ValidationError::ListLimit));
        assert_eq!(validate_list_limit(101), Err(ValidationError::ListLimit));
    }

    #[test]
    fn malformed_ed25519_blobs_are_rejected() {
        let wrong_length = STANDARD.encode([
            0, 0, 0, 11, b's', b's', b'h', b'-', b'e', b'd', b'2', b'5', b'5', b'1', b'9', 0, 0, 0,
            1, 0,
        ]);
        assert_eq!(
            validate_host_public_key(&format!("ssh-ed25519 {wrong_length}")),
            Err(ValidationError::HostPublicKey)
        );
    }
}
