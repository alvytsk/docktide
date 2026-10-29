//! Image list and inspect (spec M4 FR-1, FR-5).

use bollard::models::{ImageInspect as DockerImageInspect, ImageSummary as DockerImageSummary};
use bollard::query_parameters::{ListImagesOptionsBuilder, RemoveImageOptions};
use serde::Serialize;

use crate::connection::DockerService;
use crate::error::DockerError;
use crate::inspect::KeyValue;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImageSummary {
    /// `sha256:…`
    pub id: String,
    /// Empty for a dangling image.
    pub repo_tags: Vec<String>,
    /// Unix seconds.
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at: i64,
    /// Bytes.
    #[cfg_attr(test, ts(type = "number"))]
    pub size: i64,
    /// Bytes shared with other images; unset when the Engine does not compute it.
    #[cfg_attr(test, ts(type = "number | null"))]
    pub shared_size: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImageDetails {
    pub id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    /// RFC 3339.
    pub created_at: String,
    /// Bytes.
    #[cfg_attr(test, ts(type = "number"))]
    pub size: i64,
    /// `os/architecture[/variant]`
    pub platform: String,
    pub entrypoint: Vec<String>,
    pub cmd: Vec<String>,
    pub working_dir: Option<String>,
    pub user: Option<String>,
    /// `<port>/<protocol>`, sorted.
    pub exposed_ports: Vec<String>,
    /// `KEY=value`, as Docker reports them.
    pub env: Vec<String>,
    pub labels: Vec<KeyValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ImageInspect {
    pub details: ImageDetails,
    /// Bollard's model re-serialized, with Docker's field names.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub raw: serde_json::Value,
}

impl DockerService {
    /// Tagged and dangling images, without intermediate layers; includes `SharedSize`.
    pub async fn list_images(&self) -> Result<Vec<ImageSummary>, DockerError> {
        let (endpoint, docker) = self.docker().await?;
        let options = ListImagesOptionsBuilder::new()
            .all(false)
            .shared_size(true)
            .build();
        let images = docker
            .list_images(Some(options))
            .await
            .map_err(|err| self.failed(&err, &endpoint))?;
        Ok(images.into_iter().map(ImageSummary::from_docker).collect())
    }

    pub async fn inspect_image(&self, id: &str) -> Result<ImageInspect, DockerError> {
        let (endpoint, docker) = self.docker().await?;
        let response = docker
            .inspect_image(id)
            .await
            .map_err(|err| self.failed(&err, &endpoint))?;
        let raw = serde_json::to_value(&response).map_err(|err| DockerError::Internal {
            message: format!("Could not encode the inspect result: {err}"),
        })?;
        Ok(ImageInspect {
            details: ImageDetails::from_docker(response),
            raw,
        })
    }
}

impl DockerService {
    /// Removes the image only if no container, in any state, was created from it.
    /// An image with tags is removed tag by tag (Docker refuses a by-id delete when it has
    /// several); one that is already gone counts as removed.
    pub async fn remove_image(&self, id: &str) -> Result<(), DockerError> {
        // Everything below uses the canonical id: `id` may be a short id or a tag.
        let (id, tags) = match self.inspect_image(id).await {
            Ok(image) => (image.details.id, image.details.repo_tags),
            Err(DockerError::NotFound) => return Ok(()),
            Err(err) => return Err(err),
        };
        // Matching by image id, not by the tag a container names, so a sibling tag is safe.
        let users: Vec<_> = self
            .list_containers()
            .await?
            .into_iter()
            .filter(|container| container.image_id == id)
            .map(|container| container.name)
            .collect();
        if !users.is_empty() {
            let more = users.len().saturating_sub(3);
            let mut names = users[..users.len() - more].join(", ");
            if more > 0 {
                names = format!("{names} and {more} more");
            }
            return Err(DockerError::Conflict {
                message: format!("image is used by {names}"),
            });
        }

        let targets = if tags.is_empty() {
            vec![id.clone()]
        } else {
            tags
        };
        let mut removed: Vec<&str> = Vec::new();
        for target in &targets {
            match self.remove_one(target).await {
                Ok(()) => removed.push(target),
                Err(err) if removed.is_empty() => return Err(err),
                Err(err) => {
                    return Err(DockerError::Conflict {
                        message: format!("Removed tags {}; {target}: {err}", removed.join(", ")),
                    });
                }
            }
        }
        Ok(())
    }

    /// `NotFound` counts as removed.
    async fn remove_one(&self, name: &str) -> Result<(), DockerError> {
        let (endpoint, docker) = self.docker().await?;
        let options = RemoveImageOptions {
            force: false,
            ..Default::default()
        };
        match docker.remove_image(name, Some(options), None).await {
            Ok(_) => Ok(()),
            Err(err) => match self.failed(&err, &endpoint) {
                DockerError::NotFound => Ok(()),
                other => Err(other),
            },
        }
    }
}

impl ImageSummary {
    fn from_docker(image: DockerImageSummary) -> Self {
        Self {
            id: image.id,
            repo_tags: tags(image.repo_tags),
            created_at: image.created,
            size: image.size,
            // The Engine answers -1 when it did not compute the value.
            shared_size: Some(image.shared_size).filter(|&bytes| bytes >= 0),
        }
    }
}

impl ImageDetails {
    fn from_docker(inspect: DockerImageInspect) -> Self {
        let config = inspect.config.unwrap_or_default();
        let mut platform = format!(
            "{}/{}",
            inspect.os.unwrap_or_default(),
            inspect.architecture.unwrap_or_default()
        );
        if let Some(variant) = inspect.variant.filter(|v| !v.is_empty()) {
            platform = format!("{platform}/{variant}");
        }
        let mut exposed_ports = config.exposed_ports.unwrap_or_default();
        exposed_ports.sort();
        let mut labels: Vec<_> = config
            .labels
            .unwrap_or_default()
            .into_iter()
            .map(|(key, value)| KeyValue { key, value })
            .collect();
        labels.sort_by(|a, b| a.key.cmp(&b.key));
        Self {
            id: inspect.id.unwrap_or_default(),
            repo_tags: tags(inspect.repo_tags.unwrap_or_default()),
            repo_digests: inspect.repo_digests.unwrap_or_default(),
            created_at: inspect.created.unwrap_or_default(),
            size: inspect.size.unwrap_or_default(),
            platform,
            entrypoint: config.entrypoint.unwrap_or_default(),
            cmd: config.cmd.unwrap_or_default(),
            working_dir: config.working_dir.filter(|v| !v.is_empty()),
            user: config.user.filter(|v| !v.is_empty()),
            exposed_ports,
            env: config.env.unwrap_or_default(),
            labels,
        }
    }
}

/// Docker lists a dangling image as `<none>:<none>`.
fn tags(tags: impl IntoIterator<Item = String>) -> Vec<String> {
    tags.into_iter()
        .filter(|tag| tag != "<none>:<none>")
        .collect()
}

#[cfg(test)]
mod tests {
    use bollard::models::ImageConfig;

    use super::*;

    fn summary(tags: &[&str], shared_size: i64) -> ImageSummary {
        ImageSummary::from_docker(DockerImageSummary {
            id: "sha256:abc".into(),
            repo_tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            shared_size,
            ..Default::default()
        })
    }

    fn details(os: &str, arch: &str, variant: Option<&str>) -> ImageDetails {
        ImageDetails::from_docker(DockerImageInspect {
            os: Some(os.into()),
            architecture: Some(arch.into()),
            variant: variant.map(Into::into),
            ..Default::default()
        })
    }

    fn with_config(config: ImageConfig) -> ImageDetails {
        ImageDetails::from_docker(DockerImageInspect {
            config: Some(config),
            ..Default::default()
        })
    }

    #[test]
    fn summary_filters_none_tags() {
        assert_eq!(
            summary(&["<none>:<none>"], 0).repo_tags,
            Vec::<String>::new()
        );
        assert_eq!(summary(&["a:1", "b:2"], 0).repo_tags, ["a:1", "b:2"]);
    }

    #[test]
    fn summary_unknown_shared_size_is_none() {
        assert_eq!(summary(&[], -1).shared_size, None);
        assert_eq!(summary(&[], 2).shared_size, Some(2));
    }

    #[test]
    fn details_platform_with_variant() {
        assert_eq!(
            details("linux", "arm64", Some("v8")).platform,
            "linux/arm64/v8"
        );
    }

    #[test]
    fn details_platform_without_variant() {
        assert_eq!(details("linux", "amd64", None).platform, "linux/amd64");
    }

    #[test]
    fn details_without_config() {
        let d = ImageDetails::from_docker(DockerImageInspect::default());
        assert!(d.entrypoint.is_empty() && d.cmd.is_empty() && d.env.is_empty());
        assert!(d.exposed_ports.is_empty() && d.labels.is_empty());
        assert_eq!((d.working_dir, d.user), (None, None));
    }

    #[test]
    fn details_empty_strings_are_none() {
        let d = with_config(ImageConfig {
            working_dir: Some(String::new()),
            user: Some(String::new()),
            ..Default::default()
        });
        assert_eq!((d.working_dir, d.user), (None, None));
    }

    #[test]
    fn details_ports_and_labels_sorted() {
        let d = with_config(ImageConfig {
            exposed_ports: Some(vec!["8080/tcp".into(), "80/tcp".into()]),
            labels: Some([("b".into(), "2".into()), ("a".into(), "1".into())].into()),
            ..Default::default()
        });
        assert_eq!(d.exposed_ports, ["80/tcp", "8080/tcp"]);
        let keys: Vec<_> = d.labels.iter().map(|l| l.key.as_str()).collect();
        assert_eq!(keys, ["a", "b"]);
    }
}
