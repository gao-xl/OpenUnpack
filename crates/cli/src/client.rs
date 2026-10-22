use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use openunpack_local_api::ApiCommand;
use openunpack_protocol::{AppSnapshot, JobView};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::net::TcpStream;

#[derive(Clone, Debug)]
pub struct DaemonConnection {
    pub base_url: String,
    pub token: String,
    pub port: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub base_url: String,
}

impl DaemonConnection {
    pub fn discover(data_dir_override: Option<&Path>) -> Result<Self, String> {
        let data_dir = match data_dir_override {
            Some(d) => d.to_path_buf(),
            None => default_data_dir()?,
        };

        // Try reading daemon.info first
        let info_file = data_dir.join("daemon.info");
        if info_file.is_file()
            && let Ok(content) = std::fs::read_to_string(&info_file)
            && let Ok(info) = serde_json::from_str::<DaemonInfo>(&content)
        {
            return Ok(Self {
                base_url: info.base_url,
                token: info.token,
                port: info.port,
            });
        }

        // Try reading local-api.json for settings
        let store_file = data_dir.join("local-api.json");
        let port = if store_file.is_file() {
            let content = std::fs::read_to_string(&store_file).unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
            v.get("settings")
                .and_then(|s| s.get("port"))
                .and_then(|p| p.as_u64())
                .map(|p| p as u16)
                .unwrap_or(47980)
        } else {
            47980
        };

        let token = std::env::var("OPENUNPACK_TOKEN").unwrap_or_default();
        Ok(Self {
            base_url: format!("http://127.0.0.1:{port}"),
            token,
            port,
        })
    }

    pub async fn send_command(&self, cmd: ApiCommand) -> Result<serde_json::Value, String> {
        let body_json =
            serde_json::to_string(&cmd).map_err(|e| format!("Failed to serialize command: {e}"))?;
        let res_json = self
            .request_json("POST", "/api/v1/commands", Some(&body_json))
            .await?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawResponse {
            data: serde_json::Value,
        }

        let api_res: RawResponse = serde_json::from_value(res_json)
            .map_err(|e| format!("Invalid API response format: {e}"))?;
        Ok(api_res.data)
    }

    pub async fn get_status(&self) -> Result<serde_json::Value, String> {
        let mut status = self.send_command(ApiCommand::QueueSnapshot).await?;
        let object = status
            .as_object_mut()
            .ok_or("Invalid queue status response")?;
        object.insert("port".into(), self.port.into());
        Ok(status)
    }

    pub async fn get_snapshot(&self) -> Result<AppSnapshot, String> {
        let val = self.send_command(ApiCommand::QueueSnapshot).await?;
        serde_json::from_value(val).map_err(|e| format!("Failed to parse snapshot: {e}"))
    }

    pub async fn add_archive(
        &self,
        path: &str,
        output_dir: Option<&str>,
        password: Option<&str>,
        auto_start: bool,
    ) -> Result<Vec<JobView>, String> {
        let cmd = ApiCommand::AddArchive {
            path: path.to_string(),
            output_dir: output_dir.map(ToString::to_string),
            password: password.map(ToString::to_string),
            delete_source: None,
            auto_start,
        };

        let val = self.send_command(cmd).await?;
        serde_json::from_value(val).map_err(|e| format!("Failed to parse added jobs: {e}"))
    }

    pub async fn cancel_job(&self, job_id: &str) -> Result<(), String> {
        self.send_command(ApiCommand::CancelJobs {
            job_id: Some(job_id.to_string()),
        })
        .await?;
        Ok(())
    }

    pub async fn retry_job(&self, job_id: &str, password: Option<&str>) -> Result<(), String> {
        if let Some(pwd) = password {
            self.send_command(ApiCommand::SubmitInput {
                job_id: job_id.to_string(),
                input_type: "password".into(),
                value: pwd.to_string(),
            })
            .await?;
        } else {
            self.send_command(ApiCommand::RetryJob {
                job_id: job_id.to_string(),
                partial: false,
                password: None,
                preserve_options: true,
            })
            .await?;
        }
        Ok(())
    }

    pub async fn pause_jobs(&self) -> Result<(), String> {
        self.send_command(ApiCommand::PauseJobs).await?;
        Ok(())
    }

    pub async fn resume_jobs(&self) -> Result<(), String> {
        self.send_command(ApiCommand::StartJobs).await?;
        Ok(())
    }

    async fn request_json(
        &self,
        method: &str,
        path_and_query: &str,
        body_opt: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.request_json_inner(method, path_and_query, body_opt),
        )
        .await
        .map_err(|_| "OpenUnpack API request timed out".to_string())?
    }

    async fn request_json_inner(
        &self,
        method: &str,
        path_and_query: &str,
        body_opt: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let addr = format!("127.0.0.1:{}", self.port);
        let stream = TcpStream::connect(&addr).await.map_err(|e| {
            format!("Cannot connect to OpenUnpack daemon at {addr}: {e} (Is daemon running?)")
        })?;
        let io = TokioIo::new(stream);

        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| format!("HTTP handshake failed: {e}"))?;

        tokio::spawn(async move {
            if let Err(err) = conn.await {
                eprintln!("Connection warning: {:?}", err);
            }
        });

        let mut req_builder = Request::builder()
            .method(match method {
                "GET" => Method::GET,
                "POST" => Method::POST,
                _ => Method::GET,
            })
            .uri(path_and_query)
            .header("Host", format!("127.0.0.1:{}", self.port))
            .header("User-Agent", "OpenUnpack-CLI/0.1.0");

        if !self.token.is_empty() {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", self.token));
        }

        let body_bytes = body_opt.unwrap_or("").as_bytes().to_vec();
        if !body_bytes.is_empty() {
            req_builder = req_builder
                .header("Content-Type", "application/json")
                .header("Content-Length", body_bytes.len().to_string());
        }

        let req = req_builder
            .body(Full::new(Bytes::from(body_bytes)))
            .map_err(|e| format!("Failed to build request: {e}"))?;

        let resp = sender
            .send_request(req)
            .await
            .map_err(|e| format!("HTTP request error: {e}"))?;

        let status = resp.status();
        let mut incoming = resp.into_body();
        let mut body = Vec::new();
        while let Some(frame) = incoming.frame().await {
            let frame = frame.map_err(|_| "Failed to read API response body".to_string())?;
            if let Some(bytes) = frame.data_ref() {
                if body.len().saturating_add(bytes.len()) > 20 * 1024 * 1024 {
                    return Err("API response exceeds the size limit".into());
                }
                body.extend_from_slice(bytes);
            }
        }

        let text = String::from_utf8_lossy(&body);
        if !status.is_success() {
            return Err(format!("API Error ({})", status));
        }

        serde_json::from_str(&text).map_err(|e| format!("Failed to parse response JSON: {e}"))
    }
}

pub fn default_data_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("OPENUNPACK_DATA_DIR")
        && !dir.trim().is_empty()
    {
        return Ok(PathBuf::from(dir));
    }

    #[cfg(windows)]
    {
        if let Ok(dir) = std::env::var("LOCALAPPDATA")
            && !dir.trim().is_empty()
        {
            return Ok(PathBuf::from(dir).join("OpenUnpack"));
        }
        if let Some(local_app_data) = dirs::data_local_dir() {
            return Ok(local_app_data.join("OpenUnpack"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            return Ok(home.join("Library/Application Support/OpenUnpack"));
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        if let Ok(home) = std::env::var("HOME") {
            return Ok(PathBuf::from(home).join(".local/share/openunpack"));
        }
        if let Some(config_dir) = dirs::config_dir() {
            return Ok(config_dir.join("openunpack"));
        }
    }

    Err("Could not determine standard data directory".to_string())
}
