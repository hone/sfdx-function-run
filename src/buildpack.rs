use semver::Version;
use serde::Deserialize;
use std::fmt;
use thiserror::Error;

const REGISTRY_INDEX_HOST: &str = "https://raw.githubusercontent.com";
const REGISTRY_INDEX_PATH: &str = "buildpacks/registry-index/main";

/// Entry in the Buildpack Registry
///
/// The schema can be found
/// [here](https://github.com/buildpacks/spec/blob/extensions/buildpack-registry/0.1/extensions/buildpack-registry.md).
#[derive(Deserialize)]
pub struct RegistryEntry {
    #[serde(rename = "ns")]
    pub namespace: String,
    pub name: String,
    pub version: Version,
    pub yanked: bool,
    #[serde(rename = "addr")]
    pub address: String,
}

#[derive(Error, Debug)]
pub enum Error {
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
    pub async fn fetch(&self) -> Result<Vec<RegistryEntry>, Error> {
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

        let entries = buildpack.fetch().await.unwrap();

        assert_eq!(18, entries.len());
        assert_eq!(semver::Version::new(0, 1, 0), entries[0].version);
        registry_mock.assert();
    }
}
