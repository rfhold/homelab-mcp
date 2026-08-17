use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::DeployError;

const MAX_CATALOG_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    version: u8,
    deploys: Vec<DeployEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeployEntry {
    id: String,
    description: String,
    entrypoint: PathBuf,
    inventory: PathBuf,
    risk: Risk,
    local_available: bool,
    mcp_available: bool,
    supported_distributions: Vec<String>,
    timeout_seconds: u64,
    requires_sudo: bool,
    mutating: bool,
}

#[derive(Clone, Debug, Serialize, JsonSchema, Eq, PartialEq)]
pub struct DeployMetadata {
    pub id: String,
    pub description: String,
    pub risk: Risk,
    pub local_available: bool,
    pub mcp_available: bool,
    pub supported_distributions: Vec<String>,
    pub timeout_seconds: u64,
    pub requires_sudo: bool,
    pub mutating: bool,
}

#[derive(Clone)]
pub struct DeployCatalog {
    root: Arc<PathBuf>,
    entries: Arc<Vec<DeployEntry>>,
}

impl DeployCatalog {
    pub fn load(root: PathBuf, catalog: PathBuf) -> Result<Self, DeployError> {
        if !root.is_absolute() || !catalog.is_absolute() {
            return Err(DeployError::InvalidConfiguration);
        }
        let root = root
            .canonicalize()
            .map_err(|_| DeployError::InvalidCatalog)?;
        let metadata = fs::metadata(&catalog).map_err(|_| DeployError::InvalidCatalog)?;
        if !metadata.is_file() || metadata.len() > MAX_CATALOG_BYTES {
            return Err(DeployError::InvalidCatalog);
        }
        let bytes = fs::read(catalog).map_err(|_| DeployError::InvalidCatalog)?;
        let parsed: CatalogFile =
            serde_json::from_slice(&bytes).map_err(|_| DeployError::InvalidCatalog)?;
        if parsed.version != 1 || parsed.deploys.is_empty() || parsed.deploys.len() > 64 {
            return Err(DeployError::InvalidCatalog);
        }
        for (index, entry) in parsed.deploys.iter().enumerate() {
            if !valid_id(&entry.id)
                || entry.description.is_empty()
                || entry.description.len() > 512
                || entry.timeout_seconds == 0
                || entry.timeout_seconds > 900
                || entry.supported_distributions.is_empty()
                || parsed.deploys[..index]
                    .iter()
                    .any(|other| other.id == entry.id)
            {
                return Err(DeployError::InvalidCatalog);
            }
            confine_file(&root, &entry.entrypoint)?;
            confine_file(&root, &entry.inventory)?;
        }
        Ok(Self {
            root: Arc::new(root),
            entries: Arc::new(parsed.deploys),
        })
    }

    pub fn list(&self) -> Vec<DeployMetadata> {
        self.entries.iter().map(DeployEntry::metadata).collect()
    }

    pub(crate) fn resolve(&self, id: &str) -> Result<ResolvedDeploy, DeployError> {
        if !valid_id(id) {
            return Err(DeployError::DeployNotFound);
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or(DeployError::DeployNotFound)?;
        if !entry.mcp_available {
            return Err(DeployError::DeployUnavailable);
        }
        Ok(ResolvedDeploy {
            root: self.root.as_ref().clone(),
            entrypoint: entry.entrypoint.clone(),
            inventory: entry.inventory.clone(),
            timeout_seconds: entry.timeout_seconds,
        })
    }
}

impl DeployEntry {
    fn metadata(&self) -> DeployMetadata {
        DeployMetadata {
            id: self.id.clone(),
            description: self.description.clone(),
            risk: self.risk,
            local_available: self.local_available,
            mcp_available: self.mcp_available,
            supported_distributions: self.supported_distributions.clone(),
            timeout_seconds: self.timeout_seconds,
            requires_sudo: self.requires_sudo,
            mutating: self.mutating,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ResolvedDeploy {
    pub root: PathBuf,
    pub entrypoint: PathBuf,
    pub inventory: PathBuf,
    pub timeout_seconds: u64,
}

fn confine_file(root: &Path, relative: &Path) -> Result<(), DeployError> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(DeployError::InvalidCatalog);
    }
    let canonical = root
        .join(relative)
        .canonicalize()
        .map_err(|_| DeployError::InvalidCatalog)?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err(DeployError::InvalidCatalog);
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temporary() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "homelab-deploy-catalog-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn repository_catalog_is_strict_and_only_system_info_is_mcp_available() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let catalog = DeployCatalog::load(root.clone(), root.join("deploys/catalog.json")).unwrap();
        let entries = catalog.list();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            catalog.resolve("bootstrap-homelab").unwrap_err(),
            DeployError::DeployUnavailable
        );
        assert_eq!(catalog.resolve("system-info").unwrap().timeout_seconds, 30);
        assert_eq!(
            catalog.resolve("../../bin/sh").unwrap_err(),
            DeployError::DeployNotFound
        );
    }

    #[test]
    fn unknown_fields_and_paths_outside_root_are_rejected() {
        let root = temporary();
        fs::write(root.join("inventory.py"), "").unwrap();
        fs::write(root.join("entrypoint.py"), "").unwrap();
        let base = serde_json::json!({"version":1,"deploys":[{
            "id":"system-info","description":"test","entrypoint":"entrypoint.py","inventory":"inventory.py",
            "risk":"low","local_available":true,"mcp_available":true,"supported_distributions":["debian"],
            "timeout_seconds":1,"requires_sudo":false,"mutating":false,"extra":true
        }]});
        fs::write(
            root.join("catalog.json"),
            serde_json::to_vec(&base).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            DeployCatalog::load(root.clone(), root.join("catalog.json")),
            Err(DeployError::InvalidCatalog)
        ));

        let mut outside = base;
        outside["deploys"][0]
            .as_object_mut()
            .unwrap()
            .remove("extra");
        outside["deploys"][0]["entrypoint"] = serde_json::json!("../outside.py");
        fs::write(
            root.join("catalog.json"),
            serde_json::to_vec(&outside).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            DeployCatalog::load(root.clone(), root.join("catalog.json")),
            Err(DeployError::InvalidCatalog)
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
