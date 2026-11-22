//! Release-backed compose plugins.

use crate::{parse_version, validate_pinned_image, Error, Template};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    #[serde(flatten)]
    pub template: Template,
    pub repo: String,
    #[serde(default = "descriptor_asset")]
    pub descriptor_asset: String,
    pub image_variable: String,
    pub data_volume: String,
    pub health_path: String,
}

fn descriptor_asset() -> String {
    "rudder-plugin.json".into()
}

impl Plugin {
    pub fn parse(text: &str) -> Result<Self, Error> {
        let plugin: Self = serde_yaml_ng::from_str(text)
            .map_err(|error| Error::Validation(format!("invalid plugin: {error}")))?;
        plugin.template.validate()?;
        let parts: Vec<_> = plugin.repo.split('/').collect();
        if parts.len() != 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || *part == "."
                    || *part == ".."
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
        {
            return Err(Error::Validation("invalid plugin repository".into()));
        }
        if !plugin
            .template
            .variables
            .iter()
            .any(|v| v.name == plugin.image_variable && v.env_name.is_none())
        {
            return Err(Error::Validation(
                "plugin image variable must be declared".into(),
            ));
        }
        let compose: serde_json::Value = serde_yaml_ng::from_str(&plugin.template.compose)
            .map_err(|error| Error::Validation(format!("invalid compose: {error}")))?;
        if compose
            .get("volumes")
            .and_then(|v| v.get(&plugin.data_volume))
            .is_none()
        {
            return Err(Error::Validation("plugin data volume must exist".into()));
        }
        if !plugin.health_path.starts_with('/') || plugin.health_path.contains(['\r', '\n']) {
            return Err(Error::Validation("invalid plugin health path".into()));
        }
        Ok(plugin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub schema: u32,
    pub plugin: String,
    pub version: String,
    pub image: String,
    pub health_path: String,
    pub port: u16,
}

impl PluginDescriptor {
    pub fn verify(&self, plugin: &Plugin, tag: &str) -> Result<(), Error> {
        let version = parse_version(tag)?;
        if self.schema != 1
            || self.plugin != plugin.template.slug
            || self.version != tag.strip_prefix('v').unwrap_or(tag)
            || !version.pre.is_empty()
            || self.health_path != plugin.health_path
            || self.port != plugin.template.port
        {
            return Err(Error::Validation(
                "plugin descriptor does not match release or manifest".into(),
            ));
        }
        validate_pinned_image(&self.image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MANIFEST: &str = include_str!("../../../plugins/dockside.yml");
    #[test]
    fn shipped_manifest_and_descriptor_are_valid() {
        let plugin = Plugin::parse(MANIFEST).unwrap();
        let mut descriptor = PluginDescriptor {
            schema: 1,
            plugin: "dockside".into(),
            version: "0.2.0".into(),
            image: format!(
                "ghcr.io/vantanminh/dockside-server@sha256:{}",
                "a".repeat(64)
            ),
            health_path: "/v1/health".into(),
            port: 7420,
        };
        assert!(descriptor.verify(&plugin, "v0.2.0").is_ok());
        descriptor.version = "0.1.0".into();
        assert!(descriptor.verify(&plugin, "v0.2.0").is_err());
        descriptor.version = "0.2.0".into();
        descriptor.plugin = "other".into();
        assert!(descriptor.verify(&plugin, "v0.2.0").is_err());
        descriptor.plugin = "dockside".into();
        descriptor.image = "nginx:latest".into();
        assert!(descriptor.verify(&plugin, "v0.2.0").is_err());
    }
    #[test]
    fn manifest_requires_image_variable_and_volume() {
        assert!(Plugin::parse(
            &MANIFEST.replace("image_variable: PLUGIN_IMAGE", "image_variable: MISSING")
        )
        .is_err());
        assert!(
            Plugin::parse(&MANIFEST.replace("data_volume: data", "data_volume: missing")).is_err()
        );
    }
    #[test]
    fn text_is_encoded_and_empty_secrets_are_not_generated() {
        let plugin = Plugin::parse(MANIFEST).unwrap();
        let mut values = std::collections::HashMap::from([
            ("DOMAIN".into(), "dock.example.com".into()),
            ("PLUGIN_IMAGE".into(), "image".into()),
            ("DOCKSIDE_CONFIG".into(), "[repo]\nname = \"demo\"\n".into()),
        ]);
        let resolved = plugin
            .template
            .resolve(&values, |len| "x".repeat(len))
            .unwrap();
        assert!(resolved.env.contains(&(
            "DOCKSIDE_CONFIG_B64".into(),
            "W3JlcG9dCm5hbWUgPSAiZGVtbyIK".into()
        )));
        assert!(resolved
            .env
            .contains(&("RUDDER_TOKEN".into(), String::new())));
        values.insert("DOCKSIDE_CONFIG".into(), String::new());
        let empty = plugin
            .template
            .resolve(&values, |len| "x".repeat(len))
            .unwrap();
        assert!(empty
            .env
            .contains(&("DOCKSIDE_CONFIG_B64".into(), String::new())));
        values.insert("DOCKSIDE_RUN_VERIFY".into(), "invalid".into());
        assert!(plugin
            .template
            .resolve(&values, |len| "x".repeat(len))
            .is_err());
    }
}
