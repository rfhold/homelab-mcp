use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;

use super::DeployError;

const SECTIONS: [&str; 10] = [
    "hostname",
    "uptime",
    "boot_time",
    "os_release",
    "kernel_arch",
    "cpu",
    "memory",
    "filesystems",
    "block_devices",
    "interfaces",
];
const OPTIONAL_SECTION: &str = "default_routes";
const MAX_SECTION_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Serialize, JsonSchema, Eq, PartialEq)]
pub struct SystemInfo {
    pub sections: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "type", content = "result", rename_all = "snake_case")]
pub enum DeployResult {
    SystemInfo(SystemInfo),
}

pub(crate) fn system_info(stdout: &[u8]) -> Result<DeployResult, DeployError> {
    let text = std::str::from_utf8(stdout).map_err(|_| DeployError::InvalidOutput)?;
    let start = text
        .find("HOMELAB_SYSTEM_INFO_V1_BEGIN\n")
        .ok_or(DeployError::InvalidOutput)?
        + "HOMELAB_SYSTEM_INFO_V1_BEGIN\n".len();
    let tail = &text[start..];
    let end = tail
        .find("HOMELAB_SYSTEM_INFO_V1_END")
        .ok_or(DeployError::InvalidOutput)?;
    if tail[end + "HOMELAB_SYSTEM_INFO_V1_END".len()..].contains("HOMELAB_SYSTEM_INFO_V1_END") {
        return Err(DeployError::InvalidOutput);
    }
    let payload = &tail[..end];
    let mut sections = BTreeMap::new();
    let mut remaining = payload;
    for name in SECTIONS.into_iter().chain([OPTIONAL_SECTION]) {
        let begin = format!("--- {name} BEGIN ---\n");
        let finish = format!("\n--- {name} END ---\n");
        remaining = remaining
            .strip_prefix(&begin)
            .ok_or(DeployError::InvalidOutput)?;
        let section_end = remaining.find(&finish).ok_or(DeployError::InvalidOutput)?;
        let value = &remaining[..section_end];
        if value.len() > MAX_SECTION_BYTES || value.chars().any(|character| character == '\0') {
            return Err(DeployError::InvalidOutput);
        }
        sections.insert(name.to_owned(), value.to_owned());
        remaining = &remaining[section_end + finish.len()..];
    }
    if !remaining.is_empty() {
        return Err(DeployError::InvalidOutput);
    }
    Ok(DeployResult::SystemInfo(SystemInfo { sections }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(value: &str) -> String {
        let mut text = "ignored log\nHOMELAB_SYSTEM_INFO_V1_BEGIN\n".to_owned();
        for section in SECTIONS.into_iter().chain([OPTIONAL_SECTION]) {
            text.push_str(&format!(
                "--- {section} BEGIN ---\n{value}\n--- {section} END ---\n"
            ));
        }
        text.push_str("HOMELAB_SYSTEM_INFO_V1_END\nignored log\n");
        text
    }

    #[test]
    fn parses_only_complete_bounded_delimited_sections() {
        let result = system_info(output("safe").as_bytes()).unwrap();
        let DeployResult::SystemInfo(info) = result;
        assert_eq!(info.sections.len(), 11);
        assert!(info.sections.values().all(|value| value == "safe"));
        assert_eq!(
            system_info(b"raw pyinfra output"),
            Err(DeployError::InvalidOutput)
        );
        assert_eq!(
            system_info(output(&"x".repeat(MAX_SECTION_BYTES + 1)).as_bytes()),
            Err(DeployError::InvalidOutput)
        );
    }
}
