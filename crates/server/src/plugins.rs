//! Embedded plugin catalog and bounded GitHub release discovery.

use chrono::{DateTime, Utc};
use dymini_panel_core::{parse_version, Error, GithubRelease, Plugin, PluginDescriptor};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use utoipa::ToSchema;

#[derive(Embed)]
#[folder = "../../plugins"]
#[include = "*.yml"]
struct Files;

pub fn all() -> &'static [Plugin] {
    static PLUGINS: OnceLock<Vec<Plugin>> = OnceLock::new();
    PLUGINS.get_or_init(|| {
        let mut plugins: Vec<_> = Files::iter()
            .filter_map(|path| {
                let file = Files::get(&path)?;
                match Plugin::parse(&String::from_utf8_lossy(&file.data)) {
                    Ok(plugin) => Some(plugin),
                    Err(error) => {
                        tracing::error!(%path, %error, "invalid plugin");
                        None
                    }
                }
            })
            .collect();
        plugins.sort_by_key(|p| p.template.name.clone());
        plugins
    })
}

pub fn get(slug: &str) -> Option<&'static Plugin> {
    all().iter().find(|plugin| plugin.template.slug == slug)
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PluginRelease {
    pub version: String,
    pub image: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PluginCheck {
    pub checked_at: DateTime<Utc>,
    pub latest_version: Option<String>,
    pub releases: Vec<PluginRelease>,
    pub error: Option<String>,
}

pub async fn discover(plugin: &Plugin) -> Result<Vec<PluginRelease>, Error> {
    discover_at(plugin, "https://api.github.com").await
}

pub(crate) async fn discover_at(plugin: &Plugin, base: &str) -> Result<Vec<PluginRelease>, Error> {
    let client = crate::update::http_client()?;
    let bytes = crate::update::fetch(
        &client,
        &format!("{base}/repos/{}/releases?per_page=30", plugin.repo),
    )
    .await?;
    let releases: Vec<GithubRelease> = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Node(format!("invalid GitHub releases: {error}")))?;
    let mut verified = Vec::new();
    for release in releases.into_iter().take(30) {
        if release.draft || release.prerelease || parse_version(&release.tag_name).is_err() {
            continue;
        }
        let Some(asset) = release.asset(&plugin.descriptor_asset) else {
            continue;
        };
        if asset.size > 64 * 1024 {
            continue;
        }
        // Production downloads must remain HTTPS; the injected test server may use HTTP.
        if base == "https://api.github.com"
            && !asset
                .browser_download_url
                .starts_with("https://github.com/")
        {
            continue;
        }
        let result = async {
            let bytes = crate::update::fetch(&client, &asset.browser_download_url).await?;
            if bytes.len() > 64 * 1024 {
                return Err(Error::Validation("plugin descriptor too large".into()));
            }
            let descriptor: PluginDescriptor = serde_json::from_slice(&bytes).map_err(|error| {
                Error::Validation(format!("invalid plugin descriptor: {error}"))
            })?;
            descriptor.verify(plugin, &release.tag_name)?;
            Ok::<_, Error>(descriptor)
        }
        .await;
        match result {
            Ok(descriptor) => verified.push(PluginRelease {
                version: descriptor.version,
                image: descriptor.image,
                notes: release
                    .body
                    .unwrap_or_default()
                    .chars()
                    .take(8_000)
                    .collect(),
                published_at: release.published_at,
                html_url: release.html_url,
            }),
            Err(error) => {
                tracing::warn!(plugin = %plugin.template.slug, tag = %release.tag_name, %error, "plugin release rejected")
            }
        }
    }
    verified.sort_by(|a, b| {
        parse_version(&b.version)
            .ok()
            .cmp(&parse_version(&a.version).ok())
    });
    verified.dedup_by(|a, b| a.version == b.version);
    Ok(verified)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dockside_paths_and_hooks_are_version_guarded() {
        let plugin = super::get("dockside").unwrap();
        assert!(!plugin
            .access_paths("0.2.1")
            .iter()
            .any(|p| p == "/v1/health"));
        assert!(plugin
            .access_paths("0.2.2")
            .iter()
            .any(|p| p == "/v1/health"));
        assert!(!plugin
            .access_paths("0.2.2")
            .iter()
            .any(|p| p == "/v1/queue/drain" || p == "/"));
        assert!(plugin.backup_hook("0.2.1").is_empty());
        assert_eq!(
            plugin.backup_hook("0.2.2"),
            vec!["dockside-server", "checkpoint"]
        );
        assert!(!plugin.health_command.is_empty());
    }
    #[tokio::test]
    async fn discovery_uses_stubbed_releases_and_rejects_invalid_descriptors() {
        use axum::{routing::get, Json, Router};
        use serde_json::json;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let releases = json!([
            {"tag_name":"v0.2.0", "body":"Release notes", "assets":[{"name":"dymini-panel-plugin.json","browser_download_url": format!("{base}/good")}]},
            {"tag_name":"v0.3.0", "prerelease":true, "assets":[{"name":"dymini-panel-plugin.json","browser_download_url": format!("{base}/good")}]},
            {"tag_name":"v0.4.0", "assets":[{"name":"dymini-panel-plugin.json","browser_download_url": format!("{base}/bad")}]},
            {"tag_name":"v0.5.0", "draft":true, "assets":[]}
        ]);
        let descriptor = json!({"schema":1,"plugin":"dockside","version":"0.2.0","image":format!("ghcr.io/vantanminh/dockside-server@sha256:{}", "a".repeat(64)),"health_path":"/v1/health","port":7420});
        let router = Router::new()
            .route("/repos/vantanminh/dockside/releases", get(move || async move { Json(releases) }))
            .route("/good", get(move || async move { Json(descriptor) }))
            .route("/bad", get(|| async { Json(json!({"schema":1,"plugin":"other","version":"0.4.0","image":"nginx:latest","health_path":"/v1/health","port":7420})) }));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let result = super::discover_at(super::get("dockside").unwrap(), &base)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].version, "0.2.0");
        assert_eq!(result[0].notes, "Release notes");
        assert!(
            super::discover_at(super::get("dockside").unwrap(), &format!("{base}/missing"))
                .await
                .is_err()
        );
        server.abort();
    }

    #[test]
    fn all_shipped_manifests_are_valid() {
        for path in super::Files::iter() {
            let file = super::Files::get(&path).unwrap();
            let plugin =
                dymini_panel_core::Plugin::parse(std::str::from_utf8(&file.data).unwrap()).unwrap();
            assert_eq!(path.as_ref(), format!("{}.yml", plugin.template.slug));
        }
        assert_eq!(super::all().len(), super::Files::iter().count());
        assert!(super::get("dockside").is_some());
    }
}
