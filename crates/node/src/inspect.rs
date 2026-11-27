use std::collections::HashMap;

use bollard::query_parameters::{ListContainersOptionsBuilder, RenameContainerOptionsBuilder};
use dymini_panel_core::{validate_name, Error, CLOUDFLARED_LABEL};

use crate::LocalDriver;

/// Runtime facts only; container environment and command may contain credentials.
pub struct ContainerSnapshot {
    pub id: String,
    pub name: String,
    pub image: String,
    pub digest: Option<String>,
    pub state: String,
    pub running: bool,
    pub started_at: Option<String>,
    pub restarts: i64,
}

impl LocalDriver {
    pub async fn container_snapshot(&self, name: &str) -> Result<Option<ContainerSnapshot>, Error> {
        let docker = self.docker()?;
        let info = match docker.inspect_container(name, None).await {
            Ok(info) => info,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => return Ok(None),
            Err(error) => return Err(Self::node(error)),
        };
        let state = info.state.unwrap_or_default();
        let digest = match info.image.as_deref() {
            Some(id) => docker
                .inspect_image(id)
                .await
                .ok()
                .and_then(|image| {
                    image
                        .repo_digests
                        .and_then(|digests| digests.into_iter().next())
                })
                .or_else(|| Some(id.to_owned())),
            None => None,
        };
        Ok(Some(ContainerSnapshot {
            id: info.id.unwrap_or_default(),
            name: info
                .name
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_owned(),
            image: info
                .config
                .and_then(|config| config.image)
                .unwrap_or_default(),
            digest,
            state: state
                .status
                .map(|status| status.to_string())
                .unwrap_or_default(),
            running: state.running.unwrap_or(false)
                && !state.paused.unwrap_or(false)
                && !state.restarting.unwrap_or(false),
            started_at: state.started_at,
            restarts: info.restart_count.unwrap_or(0),
        }))
    }

    pub async fn tunnel_containers(&self) -> Result<Vec<ContainerSnapshot>, Error> {
        let options = ListContainersOptionsBuilder::default()
            .all(true)
            .filters(&HashMap::from([(
                "label",
                vec![format!("{}={}", CLOUDFLARED_LABEL.0, CLOUDFLARED_LABEL.1)],
            )]))
            .build();
        let containers = self
            .docker()?
            .list_containers(Some(options))
            .await
            .map_err(Self::node)?;
        let mut result = Vec::new();
        for container in containers {
            if let Some(id) = container.id {
                if let Some(snapshot) = self.container_snapshot(&id).await? {
                    result.push(snapshot);
                }
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }

    pub async fn rename_running_container(&self, from: &str, to: &str) -> Result<(), Error> {
        validate_name(to)?;
        let options = RenameContainerOptionsBuilder::default().name(to).build();
        self.docker()?
            .rename_container(from, options)
            .await
            .map_err(Self::node)
    }
}
