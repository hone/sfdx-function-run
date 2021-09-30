use dkregistry::{render, v2::Client};
use futures::future::try_join_all;
use semver::Version;
use serde::Deserialize;
use std::{fmt, path::Path};
use thiserror::Error;

const REGISTRY_INDEX_HOST: &str = "https://raw.githubusercontent.com";
const REGISTRY_INDEX_PATH: &str = "buildpacks/registry-index/main";

/// Entry in the Buildpack Registry
///
/// The schema can be found
/// [here](https://github.com/buildpacks/spec/blob/extensions/buildpack-registry/0.1/extensions/buildpack-registry.md).
#[derive(Deserialize)]
pub struct BuildpackRegistryEntry {
    #[serde(rename = "ns")]
    pub namespace: String,
    pub name: String,
    pub version: Version,
    pub yanked: bool,
    #[serde(rename = "addr")]
    pub address: String,
}

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("No version found: {0}")]
    NoVersionFound(semver::Version),
    #[error("Address is formatted improperly: {0}")]
    InvalidAddress(String),
    #[error("Docker Registry error: {0}")]
    DockerRegistry(#[from] DockerRegistryError),
}

#[derive(Error, Debug)]
pub enum DockerRegistryError {
    #[error("Docker Registry error: {0}")]
    DockerRegistry(#[from] dkregistry::errors::Error),
    #[error("Render error: {0}")]
    Render(#[from] dkregistry::render::RenderError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum BuildpackRegistryError {
    #[error("http request error")]
    Reqwest(#[from] reqwest::Error),
    #[error("error formatting json")]
    Json(#[from] serde_json::Error),
}

/// Buildpack
pub struct Buildpack {
    pub namespace: String,
    pub name: String,
    host: String,
}

impl fmt::Display for Buildpack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", self.namespace, self.name)
    }
}

impl Buildpack {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Buildpack {
            namespace: namespace.into(),
            name: name.into(),
            host: String::from(REGISTRY_INDEX_HOST),
        }
    }

    #[allow(dead_code)]
    fn set_host(&mut self, host: impl Into<String>) {
        self.host = host.into();
    }

    /// Fetch Registry Entries from the Buildpack Registry Index.
    pub async fn registry_entries(
        &self,
    ) -> Result<Vec<BuildpackRegistryEntry>, BuildpackRegistryError> {
        let url = format!(
            "{}/{}/{}",
            self.host,
            REGISTRY_INDEX_PATH,
            self.canonicalize_registry()
        );

        let text = reqwest::get(url).await?.text().await?;
        text.lines()
            .map(|line| Ok(serde_json::from_str(line)?))
            .collect()
    }

    /// Buildpack registry index folder structure described in [this
    /// RFC](https://github.com/buildpacks/rfcs/blob/main/text/0022-client-side-buildpack-registry.md#github-repo).
    pub fn canonicalize_registry(&self) -> String {
        if self.name.len() <= 2 {
            format!("{}/{}", self.name.len(), self)
        } else if self.name.len() == 3 {
            format!(
                "{}/{}/{}",
                self.name.len(),
                // this should never fail, since len == 3
                self.name.chars().next().unwrap(),
                self,
            )
        } else {
            format!("{}/{}/{}", &self.name[0..=1], &self.name[2..=3], self)
        }
    }
}

pub async fn download(
    entries: &Vec<BuildpackRegistryEntry>,
    version: semver::Version,
    path: impl AsRef<Path>,
) -> Result<bool, DownloadError> {
    let entry = entries
        .iter()
        .find(|entry| entry.version == version)
        .ok_or_else(|| DownloadError::NoVersionFound(version))?;

    let mut split = entry.address.split('@');
    let mut split2 = split
        .next()
        .ok_or_else(|| DownloadError::InvalidAddress(entry.address.clone()))?
        .splitn(2, '/');
    let host = split2
        .next()
        .ok_or_else(|| DownloadError::InvalidAddress(entry.address.clone()))?;
    let image = split2
        .next()
        .ok_or_else(|| DownloadError::InvalidAddress(entry.address.clone()))?;
    let reference = split
        .next()
        .ok_or_else(|| DownloadError::InvalidAddress(entry.address.clone()))?;
    download_image(host, image, reference, path).await?;

    Ok(true)
}

async fn download_image(
    host: &str,
    image: &str,
    reference: &str,
    path: impl AsRef<Path>,
) -> Result<(), DockerRegistryError> {
    let login_scope = format!("repository:{}:pull", image);
    let scopes = vec![login_scope.as_str()];
    let client = Client::configure()
        .insecure_registry(false)
        .registry(host)
        .username(None)
        .password(None)
        .build()?
        .authenticate(scopes.as_slice())
        .await?;

    println!("Fetching manifest for {}", image);

    let manifest = client.get_manifest(image, reference).await?;
    let layers_digests = manifest.layers_digests(None)?;

    println!("{} -> got {} layer(s)", &image, layers_digests.len());

    let blob_futures = layers_digests
        .iter()
        .map(|layer_digest| client.get_blob(image, layer_digest))
        .collect::<Vec<_>>();

    let blobs = try_join_all(blob_futures).await?;

    println!("Downloaded {} layers", blobs.len());

    std::fs::create_dir(&path)?;
    let can_path = path.as_ref().canonicalize()?;

    println!("Unpacking layers to {:?}", &can_path);
    render::unpack(&blobs, &can_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn it_canonicalizes_1_char() {
        assert_eq!(
            "1/test_a",
            &Buildpack::new("test", "a").canonicalize_registry()
        );
    }

    #[test]
    fn it_canonicalizes_2_chars() {
        assert_eq!(
            "2/test_ab",
            &Buildpack::new("test", "ab").canonicalize_registry()
        );
    }

    #[test]
    fn it_canonicalizes_3_chars() {
        assert_eq!(
            "3/a/test_abc",
            &Buildpack::new("test", "abc").canonicalize_registry()
        );
    }

    #[test]
    fn it_canonicalizes_4_or_more_chars() {
        assert_eq!(
            "ab/cd/test_abcde",
            &Buildpack::new("test", "abcde").canonicalize_registry()
        );
    }

    #[tokio::test]
    async fn it_fetches_registry() {
        let server = MockServer::start();
        let registry_mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/buildpacks/registry-index/main/jv/m-/heroku_jvm-function-invoker");
            then.status(200)
                .header("content-type", "text/plain; charset=utf-8")
                .body(include_str!("../fixtures/heroku_jvm-function-invoker"));
        });

        let mut buildpack = Buildpack::new("heroku", "jvm-function-invoker");
        buildpack.set_host(format!("http://{}", server.address()));

        let entries = buildpack.registry_entries().await.unwrap();

        assert_eq!(18, entries.len());
        assert_eq!(semver::Version::new(0, 1, 0), entries[0].version);
        registry_mock.assert();
    }
}
