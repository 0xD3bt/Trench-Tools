use crate::{
    AppState,
    app_logs::record_warn,
    config::NormalizedFollowLaunch,
    follow::{
        FOLLOW_RESPONSE_SCHEMA_VERSION, FollowCancelRequest, FollowDaemonClient, FollowJobRecord,
        FollowJobResponse, FollowJobState,
    },
    push_unique_warm_route,
    warm_manager::WarmControlState,
    warm_routes_for_execution,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

const FOLLOW_JOB_ACTIVITY_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

pub fn sync_follow_job_warm_state(
    warm: &mut WarmControlState,
    active_jobs: usize,
    jobs: &[FollowJobRecord],
) {
    warm.follow_jobs_active = active_jobs > 0;
    let mut routes = Vec::new();
    for job in jobs
        .iter()
        .filter(|job| matches!(job.state, FollowJobState::Armed | FollowJobState::Running))
    {
        for route in warm_routes_for_execution(&job.execution) {
            push_unique_warm_route(
                &mut routes,
                &route.provider,
                &route.endpoint_profile,
                &route.hellomoon_mev_mode,
            );
        }
    }
    warm.follow_job_routes = routes;
}

fn update_follow_job_warm_state(
    state: &Arc<AppState>,
    active_jobs: usize,
    jobs: &[FollowJobRecord],
) {
    let mut warm = state
        .warm
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    sync_follow_job_warm_state(&mut warm, active_jobs, jobs);
}

async fn refresh_follow_job_warm_state_from_daemon(state: &Arc<AppState>) {
    let client = FollowDaemonClient::new(&configured_follow_daemon_base_url());
    match client.list().await {
        Ok(response) => {
            update_follow_job_warm_state(state, response.health.activeJobs, &response.jobs)
        }
        Err(_error) => update_follow_job_warm_state(state, 0, &[]),
    }
}

pub fn spawn_follow_job_activity_refresh_task(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            refresh_follow_job_warm_state_from_daemon(&state).await;
            sleep(FOLLOW_JOB_ACTIVITY_REFRESH_INTERVAL).await;
        }
    });
}

fn configured_follow_daemon_port() -> u16 {
    std::env::var("LAUNCHDECK_FOLLOW_DAEMON_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8790)
}

pub fn configured_follow_daemon_base_url() -> String {
    std::env::var("LAUNCHDECK_FOLLOW_DAEMON_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", configured_follow_daemon_port()))
}

pub fn configured_follow_daemon_transport() -> Result<String, String> {
    let transport = std::env::var("LAUNCHDECK_FOLLOW_DAEMON_TRANSPORT")
        .unwrap_or_else(|_| "local-http".to_string())
        .trim()
        .to_lowercase();
    match transport.as_str() {
        "" | "local-http" => Ok("local-http".to_string()),
        other => Err(format!(
            "Unsupported follow daemon transport: {other}. Expected local-http."
        )),
    }
}

pub async fn follow_daemon_status_payload() -> Value {
    let base_url = configured_follow_daemon_base_url();
    match configured_follow_daemon_transport() {
        Ok(transport) => {
            let client = FollowDaemonClient::new(&base_url);
            match client.health().await {
                Ok(health) => json!({
                    "configured": true,
                    "reachable": true,
                    "transport": transport,
                    "url": base_url,
                    "health": health,
                }),
                Err(error) => json!({
                    "configured": true,
                    "reachable": false,
                    "transport": transport,
                    "url": base_url,
                    "error": error,
                }),
            }
        }
        Err(error) => json!({
            "configured": false,
            "reachable": false,
            "url": base_url,
            "error": error,
        }),
    }
}

pub async fn follow_active_jobs_count() -> u64 {
    let payload = follow_daemon_status_payload().await;
    payload
        .get("health")
        .and_then(|value| value.get("activeJobs"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

pub fn follow_daemon_browser_client() -> Result<FollowDaemonClient, String> {
    configured_follow_daemon_transport().map(|_| ())?;
    Ok(FollowDaemonClient::new(&configured_follow_daemon_base_url()))
}

pub fn attach_follow_daemon_report(
    report: &mut Value,
    transport: Option<&str>,
    reserved: Option<&FollowJobResponse>,
    armed: Option<&FollowJobResponse>,
    latest: Option<&FollowJobResponse>,
    original_follow_launch: Option<&NormalizedFollowLaunch>,
) {
    let latest_response = latest.or(armed).or(reserved);
    let mut job = latest_response.and_then(|response| response.job.clone());
    if let Some(job_record) = job.as_mut()
        && let Some(original_follow_launch) = original_follow_launch
    {
        job_record.followLaunch = original_follow_launch.clone();
    }
    let health = latest_response.map(|response| response.health.clone());
    report["followDaemon"] = json!({
        "schemaVersion": FOLLOW_RESPONSE_SCHEMA_VERSION,
        "enabled": reserved.is_some() || armed.is_some(),
        "transport": transport,
        "reserved": reserved,
        "armed": armed,
        "job": job,
        "health": health,
    });
}

pub async fn cancel_reserved_follow_job_on_launch_failure(
    client: Option<&FollowDaemonClient>,
    reserve_task: &mut Option<JoinHandle<Result<(FollowJobResponse, u128), String>>>,
    trace_id: &str,
    note: &str,
) {
    if let Some(mut task) = reserve_task.take() {
        match timeout(Duration::from_millis(1500), &mut task).await {
            Ok(_) => {}
            Err(_) => {
                task.abort();
            }
        }
    }
    let _ = cancel_follow_job_best_effort(client, trace_id, note).await;
}

pub async fn cancel_follow_job_best_effort(
    client: Option<&FollowDaemonClient>,
    trace_id: &str,
    note: &str,
) -> bool {
    if let Some(client) = client {
        let mut last_error = None;
        for attempt in 0..5 {
            match client
                .cancel(&FollowCancelRequest {
                    traceId: trace_id.to_string(),
                    actionId: None,
                    note: Some(note.to_string()),
                })
                .await
            {
                Ok(_) => {
                    return true;
                }
                Err(error) => {
                    let retry_unknown_trace = error.contains("Unknown follow job traceId");
                    last_error = Some(error);
                    if retry_unknown_trace && attempt < 4 {
                        sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                    break;
                }
            }
        }
        if let Some(error) = last_error {
            record_warn(
                "follow-client",
                "Follow daemon cancellation after launch failure was not acknowledged.".to_string(),
                Some(json!({
                    "traceId": trace_id,
                    "message": error,
                    "note": note,
                })),
            );
        }
    }
    false
}
