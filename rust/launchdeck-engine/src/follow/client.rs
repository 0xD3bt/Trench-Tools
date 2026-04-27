use super::contract::{
    FollowArmRequest, FollowCancelRequest, FollowDaemonHealth, FollowJobResponse,
    FollowReadyRequest, FollowReadyResponse, FollowReserveRequest, FollowStopAllRequest,
};
use crate::app_logs::{record_error, record_info, record_warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shared_auth::default_token_file_path;
use std::{
    fs,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct FollowDaemonClient {
    pub baseUrl: String,
    client: Client,
}

fn configured_follow_daemon_auth_token() -> Option<String> {
    // Cache the default token for a short window so every outbound follow-daemon
    // request doesn't do a disk read (and briefly race with in-place rotations).
    // We re-read on a one-second cadence so a rotated token is still picked up
    // quickly without hammering the filesystem when jobs burst.
    static CACHE: OnceLock<Mutex<Option<(Instant, Option<String>)>>> = OnceLock::new();
    const CACHE_TTL: Duration = Duration::from_secs(1);

    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let now = Instant::now();
    {
        let guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((cached_at, token)) = guard.as_ref() {
            if now.duration_since(*cached_at) < CACHE_TTL {
                return token.clone();
            }
        }
    }
    let token = fs::read_to_string(default_token_file_path())
        .ok()
        .and_then(|raw| {
            raw.lines().find_map(|line| {
                let value = line.trim();
                (!value.is_empty()).then(|| value.to_string())
            })
        });
    let mut guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some((now, token.clone()));
    token
}

impl FollowDaemonClient {
    pub fn new(base_url: &str) -> Self {
        static CLIENT: OnceLock<Client> = OnceLock::new();
        Self {
            baseUrl: base_url.trim_end_matches('/').to_string(),
            client: CLIENT.get_or_init(Client::new).clone(),
        }
    }

    fn is_readonly_poll_path(path: &str) -> bool {
        matches!(path, "/health" | "/jobs") || path.starts_with("/jobs/")
    }

    async fn request_json<TRequest, TResponse>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&TRequest>,
    ) -> Result<TResponse, String>
    where
        TRequest: Serialize + ?Sized,
        TResponse: for<'de> Deserialize<'de>,
    {
        let url = format!("{}/{}", self.baseUrl, path.trim_start_matches('/'));
        let mut request = self.client.request(method, url);
        let method_name = request
            .try_clone()
            .map(|request| {
                request
                    .build()
                    .map(|built| built.method().to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if let Some(token) = configured_follow_daemon_auth_token() {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|error| {
            let message = error.to_string();
            let details = Some(serde_json::json!({
                "baseUrl": self.baseUrl,
                "path": path,
                "method": method_name,
                "message": message,
            }));
            if Self::is_readonly_poll_path(path) {
                record_warn(
                    "follow-client",
                    format!("Follow daemon request temporarily failed: {}", path),
                    details,
                );
            } else {
                record_error(
                    "follow-client",
                    format!("Follow daemon request failed: {}", path),
                    details,
                );
            }
            message
        })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let message = format!(
                "Follow daemon request failed with status {}: {}",
                status, body
            );
            record_error(
                "follow-client",
                format!("Follow daemon request rejected: {}", path),
                Some(serde_json::json!({
                    "baseUrl": self.baseUrl,
                    "path": path,
                    "method": method_name,
                    "status": status.as_u16(),
                    "body": body,
                })),
            );
            return Err(message);
        }
        let parsed = response.json::<TResponse>().await.map_err(|error| {
            let message = error.to_string();
            record_error(
                "follow-client",
                format!("Follow daemon response decode failed: {}", path),
                Some(serde_json::json!({
                    "baseUrl": self.baseUrl,
                    "path": path,
                    "method": method_name,
                    "message": message,
                })),
            );
            message
        })?;
        if matches!(
            path,
            "/jobs/reserve" | "/jobs/arm" | "/jobs/cancel" | "/jobs/stop-all"
        ) {
            record_info(
                "follow-client",
                format!("Follow daemon request succeeded: {}", path),
                Some(serde_json::json!({
                    "baseUrl": self.baseUrl,
                    "path": path,
                    "method": method_name,
                })),
            );
        }
        Ok(parsed)
    }

    pub async fn health(&self) -> Result<FollowDaemonHealth, String> {
        self.request_json::<Value, FollowDaemonHealth>(reqwest::Method::GET, "/health", None)
            .await
    }

    pub async fn ready(&self, payload: &FollowReadyRequest) -> Result<FollowReadyResponse, String> {
        self.request_json(reqwest::Method::POST, "/ready", Some(payload))
            .await
    }

    pub async fn reserve(
        &self,
        payload: &FollowReserveRequest,
    ) -> Result<FollowJobResponse, String> {
        self.request_json(reqwest::Method::POST, "/jobs/reserve", Some(payload))
            .await
    }

    pub async fn arm(&self, payload: &FollowArmRequest) -> Result<FollowJobResponse, String> {
        self.request_json(reqwest::Method::POST, "/jobs/arm", Some(payload))
            .await
    }

    pub async fn cancel(&self, payload: &FollowCancelRequest) -> Result<FollowJobResponse, String> {
        self.request_json(reqwest::Method::POST, "/jobs/cancel", Some(payload))
            .await
    }

    pub async fn list(&self) -> Result<FollowJobResponse, String> {
        self.request_json::<Value, FollowJobResponse>(reqwest::Method::GET, "/jobs", None)
            .await
    }

    pub async fn stop_all(
        &self,
        payload: &FollowStopAllRequest,
    ) -> Result<FollowJobResponse, String> {
        self.request_json(reqwest::Method::POST, "/jobs/stop-all", Some(payload))
            .await
    }

    pub async fn status(&self, trace_id: &str) -> Result<FollowJobResponse, String> {
        self.request_json::<Value, FollowJobResponse>(
            reqwest::Method::GET,
            &format!("/jobs/{trace_id}"),
            None,
        )
        .await
    }
}
