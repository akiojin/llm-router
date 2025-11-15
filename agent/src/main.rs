//! Ollama Coordinator Agent Entry Point
#[cfg(any(target_os = "windows", target_os = "macos"))]
use ollama_coordinator_agent::gui::tray::{run_with_system_tray, TrayOptions};
use ollama_coordinator_agent::settings::{
    load_settings_from_disk, start_settings_panel, StoredSettings,
};
use ollama_coordinator_agent::{
    api, client::CoordinatorClient, logging, metrics::MetricsCollector, ollama::OllamaManager,
    registration::gpu_devices_valid,
};
use ollama_coordinator_common::{
    error::{AgentError, AgentResult},
    protocol::{HealthCheckRequest, RegisterRequest, RegisterResponse},
};
use std::{
    io::{self, Write},
    sync::Arc,
};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use tokio::runtime::Builder;
use tokio::{
    sync::Mutex,
    task::yield_now,
    time::{interval, sleep, Duration},
};
use tracing::{error, info, warn};

#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::thread;

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn main() {
    logging::init().expect("failed to initialize agent logging");
    let stored_settings = load_settings_from_disk();
    let settings_panel =
        start_settings_panel(stored_settings.clone()).expect("failed to start settings panel");
    info!("Settings panel URL: {}", settings_panel.url());

    let config = LaunchConfig::from_env_or_settings(&stored_settings);
    let tray_options = TrayOptions::new(&config.coordinator_url, settings_panel.url());

    run_with_system_tray(tray_options, move |proxy| {
        let agent_config = config.clone();
        thread::spawn(move || {
            let runtime = Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build Tokio runtime for system tray mode");
            if let Err(err) = runtime.block_on(run_agent(agent_config)) {
                error!("Agent runtime exited: {}", err);
            }
            proxy.notify_agent_exit();
        });
    });
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[tokio::main]
async fn main() {
    logging::init().expect("failed to initialize agent logging");
    let stored_settings = load_settings_from_disk();
    let settings_panel =
        start_settings_panel(stored_settings.clone()).expect("failed to start settings panel");
    info!("Settings panel URL: {}", settings_panel.url());

    let config = LaunchConfig::from_env_or_settings(&stored_settings);
    if let Err(err) = run_agent(config).await {
        error!("Agent runtime exited: {}", err);
    }
}

async fn run_agent(config: LaunchConfig) -> AgentResult<()> {
    info!("Ollama Coordinator Agent v{}", env!("CARGO_PKG_VERSION"));

    let coordinator_url = config.coordinator_url.clone();
    let ollama_port = config.ollama_port;
    let heartbeat_interval_secs = config.heartbeat_interval_secs;

    // Ollamaマネージャーを初期化
    let mut ollama_manager = OllamaManager::new(ollama_port);

    info!("Ensuring Ollama is running...");
    if let Err(e) = ollama_manager.ensure_running().await {
        error!("Failed to start Ollama: {}", e);
        return Err(e);
    }

    // マシン情報を取得
    let machine_name = resolve_machine_name();
    let ip_address = get_local_ip().unwrap_or_else(|| "127.0.0.1".parse().unwrap());
    let ollama_version = ollama_manager
        .get_version()
        .await
        .unwrap_or_else(|_| "unknown".to_string());

    info!("Machine: {}", machine_name);
    info!("IP: {}", ip_address);
    info!("Ollama version: {}", ollama_version);

    // Coordinatorクライアントを初期化
    let mut coordinator_client = CoordinatorClient::new(coordinator_url.clone());

    // メトリクスコレクターを初期化（GPU情報取得のため）
    // ollamaバイナリのパスを渡してollama psコマンドでGPU検出を可能にする
    let mut metrics_collector =
        MetricsCollector::with_ollama_path(Some(ollama_manager.ollama_path().to_path_buf()));

    // エージェント登録
    let gpu_devices = metrics_collector.gpu_devices();
    if !gpu_devices_valid(&gpu_devices) {
        error!("GPU hardware not detected or invalid. Skipping coordinator registration.");
        wait_for_user_ack(
            "GPU ハードウェアが検出できません。GPU を搭載したマシンでのみエージェントを利用できます。",
        );
        return Err(AgentError::Registration(
            "GPU hardware not detected or invalid".to_string(),
        ));
    }
    let total_gpu_count: u32 = gpu_devices.iter().map(|device| device.count).sum();
    let primary_gpu_model = gpu_devices.first().map(|device| device.model.clone());

    let register_req = RegisterRequest {
        machine_name: machine_name.clone(),
        ip_address,
        ollama_version,
        ollama_port,
        gpu_available: true,
        gpu_devices: gpu_devices.clone(),
        gpu_count: Some(total_gpu_count),
        gpu_model: primary_gpu_model,
    };

    info!(
        "Registering with Coordinator at {}...",
        coordinator_client.coordinator_url()
    );
    let register_response = match register_with_retry(&mut coordinator_client, register_req).await {
        Ok(res) => res,
        Err(e) => {
            error!(
                "Failed to register with Coordinator at {}: {}",
                coordinator_client.coordinator_url(),
                e
            );
            return Err(e);
        }
    };

    let agent_id = register_response.agent_id;
    info!("Registered successfully! Agent ID: {}", agent_id);

    // GPU能力情報を取得（静的な情報、起動時のみ）
    let gpu_capability = metrics_collector.get_gpu_capability();
    if let Some(ref capability) = gpu_capability {
        info!(
            "GPU Detected: {} (Compute {}.{}, {}MHz, {}MB, Score: {})",
            capability.model_name,
            capability.compute_capability.0,
            capability.compute_capability.1,
            capability.max_clock_mhz,
            capability.memory_total_mb,
            capability.score()
        );
    }

    // HTTPサーバーを起動（コーディネーターからのモデルプル要求を受信）
    let ollama_manager_for_api = Arc::new(Mutex::new(ollama_manager));
    let ollama_manager_clone = ollama_manager_for_api.clone();

    let agent_api_port: u16 = std::env::var("AGENT_API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(11435); // デフォルトはOllamaポート+1

    let state = api::models::AppState {
        ollama_manager: ollama_manager_for_api,
        coordinator_url: coordinator_url.clone(),
    };
    let app = api::create_router(state);
    let bind_addr = format!("0.0.0.0:{}", agent_api_port);

    info!("Starting agent HTTP server on {}", bind_addr);

    // HTTPサーバーをバックグラウンドで起動
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(&bind_addr)
            .await
            .expect("Failed to bind agent HTTP server");
        axum::serve(listener, app)
            .await
            .expect("Agent HTTP server error");
    });

    // ハートビート送信タスク
    let mut heartbeat_timer = interval(Duration::from_secs(heartbeat_interval_secs));

    info!("Starting heartbeat loop...");
    loop {
        heartbeat_timer.tick().await;

        // メトリクス収集
        let metrics = match metrics_collector.collect_metrics() {
            Ok(metrics) => metrics,
            Err(e) => {
                warn!("Failed to collect metrics: {}", e);
                continue;
            }
        };

        // ハートビート送信
        let models = {
            let ollama = ollama_manager_clone.lock().await;
            match ollama.list_models().await {
                Ok(list) => list,
                Err(err) => {
                    warn!("Failed to list Ollama models: {}", err);
                    Vec::new()
                }
            }
        };

        let heartbeat_req = HealthCheckRequest {
            agent_id,
            cpu_usage: metrics.cpu_usage,
            memory_usage: metrics.memory_usage,
            gpu_usage: metrics.gpu_usage,
            gpu_memory_usage: metrics.gpu_memory_usage,
            gpu_memory_total_mb: metrics.gpu_memory_total_mb,
            gpu_memory_used_mb: metrics.gpu_memory_used_mb,
            gpu_temperature: metrics.gpu_temperature,
            gpu_model_name: gpu_capability.as_ref().map(|c| c.model_name.clone()),
            gpu_compute_capability: gpu_capability
                .as_ref()
                .map(|c| format!("{}.{}", c.compute_capability.0, c.compute_capability.1)),
            gpu_capability_score: gpu_capability.as_ref().map(|c| c.score()),
            // エージェントはHTTPサーバーではないため、常に0を送信
            // 将来的にエージェントがHTTPサーバーとして動作する場合に実装予定
            active_requests: 0,
            average_response_time_ms: None,
            loaded_models: models,
        };

        match coordinator_client.send_heartbeat(heartbeat_req).await {
            Err(e) => warn!(
                "Failed to send heartbeat to {}: {}",
                coordinator_client.coordinator_url(),
                e
            ),
            Ok(_) => {
                if let (Some(gpu), Some(gpu_mem), Some(temp)) = (
                    metrics.gpu_usage,
                    metrics.gpu_memory_usage,
                    metrics.gpu_temperature,
                ) {
                    info!(
                        "Heartbeat sent - CPU: {:.1}%, Memory: {:.1}%, GPU: {:.1}%, GPU Memory: {:.1}%, GPU Temp: {:.1}°C",
                        metrics.cpu_usage, metrics.memory_usage, gpu, gpu_mem, temp
                    );
                } else {
                    info!(
                        "Heartbeat sent - CPU: {:.1}%, Memory: {:.1}%",
                        metrics.cpu_usage, metrics.memory_usage
                    );
                }
            }
        }
    }

    #[allow(unreachable_code)]
    {
        Ok(())
    }
}

#[derive(Clone)]
struct LaunchConfig {
    coordinator_url: String,
    ollama_port: u16,
    heartbeat_interval_secs: u64,
}

impl LaunchConfig {
    fn from_env_or_settings(stored: &StoredSettings) -> Self {
        let coordinator_url = std::env::var("COORDINATOR_URL")
            .ok()
            .or_else(|| stored.coordinator_url())
            .unwrap_or_else(|| "http://localhost:8080".to_string());

        let ollama_port = env_u16("OLLAMA_PORT")
            .or(stored.ollama_port)
            .unwrap_or(11434);

        let heartbeat_interval_secs = env_u64("AGENT_HEARTBEAT_INTERVAL_SECS")
            .or(stored.heartbeat_interval_secs)
            .unwrap_or(10);

        Self {
            coordinator_url,
            ollama_port,
            heartbeat_interval_secs,
        }
    }
}

fn env_u16(key: &str) -> Option<u16> {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
}

/// ローカルIPアドレスを取得
fn get_local_ip() -> Option<std::net::IpAddr> {
    use std::net::UdpSocket;

    // ダミーのUDP接続を作成してローカルIPを取得
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;

    Some(addr.ip())
}

fn resolve_machine_name() -> String {
    const OVERRIDE_KEYS: [&str; 2] = ["OLLAMA_AGENT_MACHINE_NAME", "OLLAMA_MACHINE_NAME"];
    for key in OVERRIDE_KEYS {
        if let Some(value) = candidate_from_env(key) {
            return value;
        }
    }

    if let Some(value) = candidate_from_env("PRETTY_HOSTNAME") {
        return value;
    }

    if let Some(value) = candidate_from_machine_info() {
        return value;
    }

    for key in ["COMPUTERNAME", "HOSTNAME"] {
        if let Some(value) = candidate_from_env(key) {
            return value;
        }
    }

    if let Ok(hostname) = whoami::fallible::hostname() {
        if let Some(normalized) = normalize_machine_name(&hostname) {
            return normalized;
        }
    }

    if let Some(normalized) = normalize_machine_name(&whoami::devicename()) {
        return normalized;
    }

    "unknown-machine".to_string()
}

fn candidate_from_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .and_then(|value| normalize_machine_name(&value))
}

fn candidate_from_machine_info() -> Option<String> {
    let content = std::fs::read_to_string("/etc/machine-info").ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("PRETTY_HOSTNAME=") {
            let cleaned = value.trim_matches('"');
            if let Some(normalized) = normalize_machine_name(cleaned) {
                return Some(normalized);
            }
        }
    }
    None
}

fn normalize_machine_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if looks_like_container_id(trimmed) {
        return None;
    }

    Some(trimmed.to_string())
}

fn looks_like_container_id(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.len() == 12 && trimmed.chars().all(|c| c.is_ascii_hexdigit())
}

fn wait_for_user_ack(message: &str) {
    eprintln!("{}", message);
    eprintln!("Enter キーを押すと終了します。");

    let _ = io::stderr().flush();
    let mut buffer = String::new();
    let _ = io::stdin().read_line(&mut buffer);
}

async fn register_with_retry(
    client: &mut CoordinatorClient,
    req: RegisterRequest,
) -> AgentResult<RegisterResponse> {
    let retry_interval = registration_retry_interval();
    let max_attempts = registration_retry_limit();
    let mut attempts = 0usize;
    let coordinator_url = client.coordinator_url().to_string();
    const GPU_HELP_MESSAGE: &str = r#"
========================================
ERROR: GPU Required
========================================
This coordinator requires agents to have GPU available.
GPU was not detected on this machine.

To run in Docker or environments where GPU detection fails,
set the following environment variables:

  OLLAMA_GPU_AVAILABLE=true
  OLLAMA_GPU_MODEL="Your GPU Model Name"
  OLLAMA_GPU_COUNT=1

Example:
  docker run -e OLLAMA_GPU_AVAILABLE=true \
             -e OLLAMA_GPU_MODEL="Apple M4" \
             -e OLLAMA_GPU_COUNT=1 \
             your-agent-image
========================================
"#;

    loop {
        attempts = attempts.saturating_add(1);
        match client.register(req.clone()).await {
            Ok(response) => return Ok(response),
            Err(err) => {
                // Check for 403 Forbidden (GPU not available)
                let err_msg = err.to_string();
                if err_msg.contains("403") || err_msg.contains("Forbidden") {
                    error!(
                        "Coordinator {} rejected registration due to GPU requirement.",
                        coordinator_url
                    );
                    error!("{}", GPU_HELP_MESSAGE.trim());
                    return Err(err);
                }

                let target = max_attempts
                    .map(|limit| limit.to_string())
                    .unwrap_or_else(|| "∞".to_string());
                warn!(
                    "Failed to register with Coordinator at {} (attempt {} of {}): {}",
                    coordinator_url, attempts, target, err
                );

                if let Some(limit) = max_attempts {
                    if attempts >= limit {
                        return Err(err);
                    }
                }

                if retry_interval.is_zero() {
                    yield_now().await;
                } else {
                    sleep(retry_interval).await;
                }
            }
        }
    }
}

fn registration_retry_interval() -> Duration {
    std::env::var("COORDINATOR_REGISTER_RETRY_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(5))
}

fn registration_retry_limit() -> Option<usize> {
    std::env::var("COORDINATOR_REGISTER_MAX_RETRIES")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .and_then(|limit| if limit == 0 { None } else { Some(limit) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ollama_coordinator_common::protocol::RegisterStatus;
    use ollama_coordinator_common::types::GpuDeviceInfo;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    };
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn new(key: &'static str, value: Option<&str>) -> Self {
            let original = std::env::var(key).ok();
            match value {
                Some(val) => std::env::set_var(key, val),
                None => std::env::remove_var(key),
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(ref value) = self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[derive(Clone, Default)]
    struct RegisterSequenceResponder {
        hits: Arc<AtomicUsize>,
    }

    impl Respond for RegisterSequenceResponder {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            let attempt = self.hits.fetch_add(1, Ordering::SeqCst);
            if attempt < 2 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "agent_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                    "status": "registered"
                }))
            }
        }
    }

    #[test]
    fn test_resolve_machine_name_override() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _guard_agent = EnvGuard::new("OLLAMA_AGENT_MACHINE_NAME", Some("override-machine"));
        assert_eq!(resolve_machine_name(), "override-machine");
    }

    #[test]
    fn test_resolve_machine_name_fallback_hostname_env() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _guard_override = EnvGuard::new("OLLAMA_AGENT_MACHINE_NAME", None);
        let _guard_machine_name = EnvGuard::new("OLLAMA_MACHINE_NAME", None);
        let _guard_pretty = EnvGuard::new("PRETTY_HOSTNAME", None);
        let _guard_host = EnvGuard::new("HOSTNAME", Some("custom-host-name"));
        #[cfg(windows)]
        let _guard_computer = EnvGuard::new("COMPUTERNAME", None);

        assert_eq!(resolve_machine_name(), "custom-host-name");
    }

    #[test]
    fn test_resolve_machine_name_pretty_hostname() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _guard_override = EnvGuard::new("OLLAMA_AGENT_MACHINE_NAME", None);
        let _guard_machine_name = EnvGuard::new("OLLAMA_MACHINE_NAME", None);
        let _guard_host = EnvGuard::new("HOSTNAME", Some("container-host"));
        let _guard_pretty = EnvGuard::new("PRETTY_HOSTNAME", Some("pretty-host-display"));

        assert_eq!(resolve_machine_name(), "pretty-host-display");
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_register_with_retry_eventual_success() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _guard_retry_secs = EnvGuard::new("COORDINATOR_REGISTER_RETRY_SECS", Some("0"));
        let _guard_retry_limit = EnvGuard::new("COORDINATOR_REGISTER_MAX_RETRIES", Some("0"));

        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/agents"))
            .respond_with(RegisterSequenceResponder::default())
            .expect(3)
            .mount(&server)
            .await;

        let mut client = CoordinatorClient::new(server.uri());
        let register_req = RegisterRequest {
            machine_name: "retry-test".to_string(),
            ip_address: "127.0.0.1".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let response = register_with_retry(&mut client, register_req)
            .await
            .expect("registration should eventually succeed");

        assert_eq!(response.status, RegisterStatus::Registered);
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_register_with_retry_respects_limit() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _guard_retry_secs = EnvGuard::new("COORDINATOR_REGISTER_RETRY_SECS", Some("0"));
        let _guard_retry_limit = EnvGuard::new("COORDINATOR_REGISTER_MAX_RETRIES", Some("2"));

        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/agents"))
            .respond_with(ResponseTemplate::new(503))
            .expect(2)
            .mount(&server)
            .await;

        let mut client = CoordinatorClient::new(server.uri());
        let register_req = RegisterRequest {
            machine_name: "retry-test".to_string(),
            ip_address: "127.0.0.1".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let result = register_with_retry(&mut client, register_req).await;
        assert!(result.is_err());
    }
}
