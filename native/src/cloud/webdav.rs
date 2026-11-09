use std::time::Duration;

use reqwest::{Client, Method, StatusCode, Url};

use crate::cloud::config::WebDavConfig;
use crate::cloud::sync::CloudSnapshot;
use crate::error::CloudError;

const SYNC_FILE: &str = "lumen-reader-sync.json";

pub struct WebDavClient {
    client: Client,
    base: Url,
    username: String,
    password: String,
    remote_dir: String,
}

impl WebDavClient {
    pub fn new(config: &WebDavConfig) -> Result<Self, CloudError> {
        if !config.configured() {
            return Err(CloudError::NotConfigured);
        }
        let mut base = Url::parse(config.url.trim())
            .map_err(|error| CloudError::InvalidUrl(format!("{}（{error}）", config.url.trim())))?;
        if base.scheme() != "http" && base.scheme() != "https" {
            return Err(CloudError::InvalidUrl(
                "只支持 http:// 或 https://".to_owned(),
            ));
        }
        let trimmed_path = base.path().trim_end_matches('/').to_owned();
        base.set_path(&trimmed_path);

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(15))
            .build()?;

        Ok(Self {
            client,
            base,
            username: config.username.trim().to_owned(),
            password: config.effective_password(),
            remote_dir: normalize_remote_dir(&config.remote_dir),
        })
    }

    fn auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.username.is_empty() {
            request
        } else {
            request.basic_auth(&self.username, Some(&self.password))
        }
    }

    fn endpoint(&self, segments: &[&str]) -> Result<Url, CloudError> {
        let mut url = self.base.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|()| CloudError::InvalidUrl("WebDAV 地址不能作为目录使用".to_owned()))?;
            for directory in self.remote_dir.split('/').filter(|part| !part.is_empty()) {
                path.push(directory);
            }
            for segment in segments {
                path.push(segment);
            }
        }
        Ok(url)
    }

    pub async fn ensure_directory(&self) -> Result<(), CloudError> {
        let parts: Vec<&str> = self
            .remote_dir
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        for end in 1..=parts.len() {
            let url = self.endpoint(&parts[..end])?;
            let method = Method::from_bytes(b"MKCOL")
                .map_err(|error| CloudError::InvalidData(error.to_string()))?;
            let response = self.auth(self.client.request(method, url)).send().await?;
            let status = response.status();
            // 405 = already exists on most WebDAV servers.
            if status.is_success() || status == StatusCode::METHOD_NOT_ALLOWED {
                continue;
            }
            return Err(status_error(status, response).await);
        }
        Ok(())
    }

    pub async fn get_snapshot(&self) -> Result<Option<CloudSnapshot>, CloudError> {
        let url = self.endpoint(&[SYNC_FILE])?;
        let response = self.auth(self.client.get(url)).send().await?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(status_error(status, response).await);
        }
        let bytes = response.bytes().await?;
        serde_json::from_slice::<CloudSnapshot>(&bytes)
            .map(Some)
            .map_err(|error| CloudError::InvalidData(error.to_string()))
    }

    pub async fn put_snapshot(&self, snapshot: &CloudSnapshot) -> Result<(), CloudError> {
        let url = self.endpoint(&[SYNC_FILE])?;
        let body = serde_json::to_vec_pretty(snapshot)
            .map_err(|error| CloudError::InvalidData(error.to_string()))?;
        let response = self
            .auth(
                self.client
                    .put(url)
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        "application/json; charset=utf-8",
                    )
                    .body(body),
            )
            .send()
            .await?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        if status == StatusCode::CONFLICT || status == StatusCode::NOT_FOUND {
            // Some servers require the collection to exist again after a
            // session reset. Recreate it once, then retry.
            self.ensure_directory().await?;
            let url = self.endpoint(&[SYNC_FILE])?;
            let body = serde_json::to_vec_pretty(snapshot)
                .map_err(|error| CloudError::InvalidData(error.to_string()))?;
            let retry = self
                .auth(
                    self.client
                        .put(url)
                        .header(
                            reqwest::header::CONTENT_TYPE,
                            "application/json; charset=utf-8",
                        )
                        .body(body),
                )
                .send()
                .await?;
            let retry_status = retry.status();
            if retry_status.is_success() {
                return Ok(());
            }
            return Err(status_error(retry_status, retry).await);
        }
        Err(status_error(status, response).await)
    }

    /// WebDAV PROPFIND on the configured root. A 404 still proves the server
    /// answered, so it is reported separately rather than as a network error.
    pub async fn test_connection(&self) -> Result<String, CloudError> {
        let url = self.endpoint(&[])?;
        let method = Method::from_bytes(b"PROPFIND")
            .map_err(|error| CloudError::InvalidData(error.to_string()))?;
        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/></d:prop></d:propfind>"#;
        let response = self
            .auth(
                self.client
                    .request(method, url)
                    .header("Depth", "0")
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        "application/xml; charset=utf-8",
                    )
                    .body(body),
            )
            .send()
            .await?;
        let status = response.status();
        match status {
            StatusCode::MULTI_STATUS | StatusCode::OK => Ok("连接成功".to_owned()),
            StatusCode::NOT_FOUND => {
                Ok("服务器可达，但远端目录尚不存在（同步时会自动创建）".to_owned())
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Err(CloudError::Unauthorized(status.as_u16()))
            }
            _ => Err(status_error(status, response).await),
        }
    }
}

fn normalize_remote_dir(input: &str) -> String {
    input
        .trim()
        .replace('\\', "/")
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .collect::<Vec<_>>()
        .join("/")
}

async fn status_error(status: StatusCode, response: reqwest::Response) -> CloudError {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return CloudError::Unauthorized(status.as_u16());
    }
    let body = response
        .text()
        .await
        .map_or_else(|_| String::new(), |text| text);
    let short: String = body.chars().take(240).collect();
    CloudError::Http(status.as_u16(), short)
}
