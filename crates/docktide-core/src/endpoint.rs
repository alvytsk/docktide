//! Which Docker daemon to talk to, following the Docker CLI's precedence (spec M0 FR-1).

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::DockerError;

pub const DEFAULT_SOCKET: &str = "unix:///var/run/docker.sock";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum EndpointSource {
    DockerHostEnv,
    DockerContextEnv,
    ConfigContext,
    Default,
    RootlessFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "EndpointInfo"))]
pub struct ResolvedEndpoint {
    pub uri: String,
    pub source: EndpointSource,
    pub context_name: Option<String>,
}

/// The file system reads resolution needs, injectable for tests.
pub trait Fs {
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn exists(&self, path: &Path) -> bool;
}

#[derive(Debug)]
pub struct RealFs;

impl Fs for RealFs {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// Resolves the endpoint from the process environment and the real file system.
pub fn resolve_from_system() -> Result<ResolvedEndpoint, DockerError> {
    resolve(|key| std::env::var(key).ok(), &RealFs)
}

/// Resolves the endpoint from injected inputs. `env` returns a variable's value.
pub fn resolve(
    env: impl Fn(&str) -> Option<String>,
    fs: &impl Fs,
) -> Result<ResolvedEndpoint, DockerError> {
    let var = |key: &str| env(key).filter(|value| !value.is_empty());

    if let Some(host) = var("DOCKER_HOST") {
        return endpoint(host, EndpointSource::DockerHostEnv, None);
    }

    let config_dir = var("DOCKER_CONFIG")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|home| Path::new(&home).join(".docker")))
        .unwrap_or_else(|| PathBuf::from("~/.docker"));
    let context = match var("DOCKER_CONTEXT") {
        Some(name) => Some((name, EndpointSource::DockerContextEnv)),
        None => current_context(fs, &config_dir)?.map(|name| (name, EndpointSource::ConfigContext)),
    };
    if let Some((name, source)) = context.filter(|(name, _)| name != "default") {
        let host = context_host(fs, &config_dir, &name)?;
        return endpoint(host, source, Some(name));
    }

    if !fs.exists(Path::new(DEFAULT_SOCKET.trim_start_matches("unix://")))
        && let Some(runtime_dir) = var("XDG_RUNTIME_DIR")
    {
        let rootless = Path::new(&runtime_dir).join("docker.sock");
        if fs.exists(&rootless) {
            let uri = format!("unix://{}", rootless.display());
            return endpoint(uri, EndpointSource::RootlessFallback, None);
        }
    }
    endpoint(DEFAULT_SOCKET.to_owned(), EndpointSource::Default, None)
}

fn endpoint(
    uri: String,
    source: EndpointSource,
    context_name: Option<String>,
) -> Result<ResolvedEndpoint, DockerError> {
    if !uri.starts_with("unix://") {
        return Err(DockerError::UnsupportedEndpoint { uri });
    }
    Ok(ResolvedEndpoint {
        uri,
        source,
        context_name,
    })
}

fn current_context(fs: &impl Fs, config_dir: &Path) -> Result<Option<String>, DockerError> {
    #[derive(Deserialize)]
    struct Config {
        #[serde(rename = "currentContext")]
        current_context: Option<String>,
    }

    let path = config_dir.join("config.json");
    let invalid = |detail: String| DockerError::InvalidConfiguration {
        context: None,
        path: path.clone(),
        detail,
    };
    let contents = match fs.read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(invalid(err.to_string())),
    };
    let config: Config = serde_json::from_str(&contents).map_err(|err| invalid(err.to_string()))?;
    Ok(config.current_context.filter(|name| !name.is_empty()))
}

/// Reads `Endpoints.docker.Host` from the context's metadata, stored under the
/// SHA-256 of its name like the Docker CLI does.
fn context_host(fs: &impl Fs, config_dir: &Path, name: &str) -> Result<String, DockerError> {
    #[derive(Deserialize)]
    struct Meta {
        #[serde(rename = "Endpoints")]
        endpoints: Endpoints,
    }
    #[derive(Deserialize)]
    struct Endpoints {
        docker: Docker,
    }
    #[derive(Deserialize)]
    struct Docker {
        #[serde(rename = "Host")]
        host: String,
    }

    let hash = Sha256::digest(name)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    let path = config_dir
        .join("contexts/meta")
        .join(hash)
        .join("meta.json");
    let invalid = |detail: String| DockerError::InvalidConfiguration {
        context: Some(name.to_owned()),
        path: path.clone(),
        detail,
    };
    let contents = fs.read_to_string(&path).map_err(|err| {
        invalid(if err.kind() == io::ErrorKind::NotFound {
            "the context does not exist".to_owned()
        } else {
            err.to_string()
        })
    })?;
    let meta: Meta = serde_json::from_str(&contents).map_err(|err| invalid(err.to_string()))?;
    Ok(meta.endpoints.docker.host)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    struct FakeFs(HashMap<PathBuf, String>);

    impl FakeFs {
        fn with(mut self, path: &str, contents: &str) -> Self {
            self.0.insert(path.into(), contents.into());
            self
        }
    }

    impl Fs for FakeFs {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.0
                .get(path)
                .cloned()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
        }

        fn exists(&self, path: &Path) -> bool {
            self.0.contains_key(path)
        }
    }

    const HOME: &str = "/home/u";
    const CONFIG: &str = "/home/u/.docker/config.json";
    // sha256("desktop-linux"), the directory name the real Docker CLI created.
    const DESKTOP_META: &str = "/home/u/.docker/contexts/meta/\
        fe9c6bd7a66301f49ca9b6a70b217107cd1284598bfc254700c989b916da791e/meta.json";
    const DESKTOP_HOST: &str = "unix:///home/user/.docker/desktop/docker.sock";
    // Written by `docker context create desktop-linux --docker host=...`.
    const DESKTOP_META_JSON: &str = include_str!("../testdata/desktop-linux.meta.json");

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    fn endpoint(uri: &str, source: EndpointSource, context: Option<&str>) -> ResolvedEndpoint {
        ResolvedEndpoint {
            uri: uri.into(),
            source,
            context_name: context.map(Into::into),
        }
    }

    fn desktop_fs() -> FakeFs {
        FakeFs::default().with(DESKTOP_META, DESKTOP_META_JSON)
    }

    #[test]
    fn docker_host_wins_over_everything() {
        let fs = desktop_fs().with(CONFIG, r#"{"currentContext":"desktop-linux"}"#);
        let vars = [
            ("HOME", HOME),
            ("DOCKER_HOST", "unix:///run/custom.sock"),
            ("DOCKER_CONTEXT", "desktop-linux"),
        ];
        assert_eq!(
            resolve(env(&vars), &fs).unwrap(),
            endpoint(
                "unix:///run/custom.sock",
                EndpointSource::DockerHostEnv,
                None
            )
        );
    }

    #[test]
    fn empty_docker_host_is_ignored() {
        let fs = FakeFs::default().with("/var/run/docker.sock", "");
        let vars = [("HOME", HOME), ("DOCKER_HOST", "")];
        assert_eq!(
            resolve(env(&vars), &fs).unwrap(),
            endpoint(DEFAULT_SOCKET, EndpointSource::Default, None)
        );
    }

    #[test]
    fn docker_context_env_wins_over_config() {
        let fs = desktop_fs().with(CONFIG, r#"{"currentContext":"other"}"#);
        let vars = [("HOME", HOME), ("DOCKER_CONTEXT", "desktop-linux")];
        assert_eq!(
            resolve(env(&vars), &fs).unwrap(),
            endpoint(
                DESKTOP_HOST,
                EndpointSource::DockerContextEnv,
                Some("desktop-linux")
            )
        );
    }

    #[test]
    fn config_current_context_is_used() {
        let fs = desktop_fs().with(CONFIG, r#"{"currentContext":"desktop-linux"}"#);
        assert_eq!(
            resolve(env(&[("HOME", HOME)]), &fs).unwrap(),
            endpoint(
                DESKTOP_HOST,
                EndpointSource::ConfigContext,
                Some("desktop-linux")
            )
        );
    }

    #[test]
    fn docker_config_overrides_home() {
        let fs = FakeFs::default()
            .with(
                "/etc/dc/config.json",
                r#"{"currentContext":"desktop-linux"}"#,
            )
            .with(
                "/etc/dc/contexts/meta/\
                 fe9c6bd7a66301f49ca9b6a70b217107cd1284598bfc254700c989b916da791e/meta.json",
                DESKTOP_META_JSON,
            );
        let vars = [("HOME", HOME), ("DOCKER_CONFIG", "/etc/dc")];
        assert_eq!(resolve(env(&vars), &fs).unwrap().uri, DESKTOP_HOST);
    }

    #[test]
    fn default_context_name_means_default_socket() {
        let fs = desktop_fs()
            .with(CONFIG, r#"{"currentContext":"desktop-linux"}"#)
            .with("/var/run/docker.sock", "");
        // DOCKER_CONTEXT=default overrides the config, as in the Docker CLI.
        let vars = [("HOME", HOME), ("DOCKER_CONTEXT", "default")];
        assert_eq!(
            resolve(env(&vars), &fs).unwrap(),
            endpoint(DEFAULT_SOCKET, EndpointSource::Default, None)
        );

        let fs = FakeFs::default().with(CONFIG, r#"{"currentContext":"default"}"#);
        assert_eq!(
            resolve(env(&[("HOME", HOME)]), &fs).unwrap().source,
            EndpointSource::Default
        );
    }

    #[test]
    fn config_without_current_context_means_default() {
        let fs = FakeFs::default().with(CONFIG, r#"{"auths":{}}"#);
        assert_eq!(
            resolve(env(&[("HOME", HOME)]), &fs).unwrap().source,
            EndpointSource::Default
        );
    }

    #[test]
    fn missing_context_is_invalid_configuration() {
        let fs = FakeFs::default().with(CONFIG, r#"{"currentContext":"gone"}"#);
        let err = resolve(env(&[("HOME", HOME)]), &fs).unwrap_err();
        assert!(matches!(
            err,
            DockerError::InvalidConfiguration { context: Some(ref name), ref path, .. }
                if name == "gone" && path.ends_with("meta.json")
        ));
    }

    #[test]
    fn malformed_config_is_invalid_configuration() {
        let fs = FakeFs::default().with(CONFIG, "{not json");
        let err = resolve(env(&[("HOME", HOME)]), &fs).unwrap_err();
        assert!(matches!(
            err,
            DockerError::InvalidConfiguration { context: None, ref path, .. }
                if path == Path::new(CONFIG)
        ));
    }

    #[test]
    fn malformed_context_metadata_is_invalid_configuration() {
        let fs = FakeFs::default().with(DESKTOP_META, r#"{"Endpoints":{}}"#);
        let vars = [("HOME", HOME), ("DOCKER_CONTEXT", "desktop-linux")];
        assert!(matches!(
            resolve(env(&vars), &fs).unwrap_err(),
            DockerError::InvalidConfiguration {
                context: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn non_unix_docker_host_is_unsupported() {
        let vars = [("DOCKER_HOST", "tcp://127.0.0.1:2375")];
        assert!(matches!(
            resolve(env(&vars), &FakeFs::default()).unwrap_err(),
            DockerError::UnsupportedEndpoint { uri } if uri == "tcp://127.0.0.1:2375"
        ));
    }

    #[test]
    fn non_unix_context_host_is_unsupported() {
        let meta = r#"{"Endpoints":{"docker":{"Host":"ssh://me@box"}}}"#;
        let fs = FakeFs::default().with(DESKTOP_META, meta);
        let vars = [("HOME", HOME), ("DOCKER_CONTEXT", "desktop-linux")];
        assert!(matches!(
            resolve(env(&vars), &fs).unwrap_err(),
            DockerError::UnsupportedEndpoint { .. }
        ));
    }

    #[test]
    fn rootless_socket_is_the_fallback_when_default_is_missing() {
        let fs = FakeFs::default().with("/run/user/1000/docker.sock", "");
        let vars = [("HOME", HOME), ("XDG_RUNTIME_DIR", "/run/user/1000")];
        assert_eq!(
            resolve(env(&vars), &fs).unwrap(),
            endpoint(
                "unix:///run/user/1000/docker.sock",
                EndpointSource::RootlessFallback,
                None
            )
        );
    }

    #[test]
    fn default_socket_wins_over_rootless() {
        let fs = FakeFs::default()
            .with("/var/run/docker.sock", "")
            .with("/run/user/1000/docker.sock", "");
        let vars = [("XDG_RUNTIME_DIR", "/run/user/1000")];
        assert_eq!(
            resolve(env(&vars), &fs).unwrap().source,
            EndpointSource::Default
        );
    }

    #[test]
    fn no_socket_anywhere_still_resolves_to_default() {
        let vars = [("XDG_RUNTIME_DIR", "/run/user/1000")];
        assert_eq!(
            resolve(env(&vars), &FakeFs::default()).unwrap(),
            endpoint(DEFAULT_SOCKET, EndpointSource::Default, None)
        );
    }
}
