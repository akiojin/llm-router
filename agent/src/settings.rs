use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc::{self, Sender},
    thread,
};

use axum::{
    extract::{Form, State},
    http::StatusCode,
    response::Html,
    routing::get,
    Router,
};
use serde::{
    de::{self, Deserializer},
    Deserialize, Serialize,
};
use tokio::{net::TcpListener, runtime::Builder};

use ollama_coordinator_common::error::AgentError;
use tracing::error;

const SETTINGS_FILE_NAME: &str = "agent-settings.json";
const SETTINGS_SUBTITLE: &str = "変更を保存後、エージェントを再起動すると反映されます。";

/// 永続化されたエージェント設定（次回起動時に復元される）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredSettings {
    /// コーディネーターのベースURL。
    pub coordinator_url: Option<String>,
    /// ローカルOllamaのポート番号。
    pub ollama_port: Option<u16>,
    /// ハートビート送信間隔（秒）。
    pub heartbeat_interval_secs: Option<u64>,
}

impl StoredSettings {
    /// 保存済みのコーディネーターURLを取得する。
    pub fn coordinator_url(&self) -> Option<String> {
        self.coordinator_url.clone()
    }
}

#[derive(Clone)]
struct AppState {
    settings_path: PathBuf,
}

#[derive(Deserialize, Debug)]
struct SettingsFormInput {
    coordinator_url: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_u16")]
    ollama_port: Option<u16>,
    #[serde(default, deserialize_with = "deserialize_opt_u64")]
    heartbeat_interval_secs: Option<u64>,
}

/// 起動済み設定パネルのハンドル。
pub struct SettingsPanelHandle {
    url: String,
}

impl SettingsPanelHandle {
    /// ブラウザでアクセス可能な設定パネルURL。
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// ディスクに保存されている設定を読み込む（存在しなければ空の設定を返す）。
pub fn load_settings_from_disk() -> StoredSettings {
    settings_file_path()
        .ok()
        .and_then(|path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str::<StoredSettings>(&content).ok())
        })
        .unwrap_or_default()
}

/// ローカル設定パネルを起動し、トレイやログからアクセスできるようにする。
pub fn start_settings_panel(initial: StoredSettings) -> Result<SettingsPanelHandle, AgentError> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let runtime = match Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(err) => {
                let _ = tx.send(Err(AgentError::Gui(format!(
                    "Failed to init settings runtime: {err}"
                ))));
                return;
            }
        };

        runtime.block_on(run_settings_server(initial, tx.clone()));
    });

    let url = rx
        .recv()
        .map_err(|_| AgentError::Gui("Failed to start settings panel".to_string()))??;

    Ok(SettingsPanelHandle { url })
}

async fn run_settings_server(
    initial: StoredSettings,
    ready_tx: Sender<Result<String, AgentError>>,
) {
    let path = match settings_file_path() {
        Ok(path) => path,
        Err(err) => {
            let _ = ready_tx.send(Err(err));
            return;
        }
    };
    if let Err(err) = persist_settings(&path, &initial) {
        let _ = ready_tx.send(Err(err));
        return;
    }

    let app_state = AppState {
        settings_path: path.clone(),
    };

    let router = Router::new()
        .route("/", get(settings_page).post(save_settings))
        .with_state(app_state);

    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(err) => {
            let err = AgentError::Gui(format!("Failed to bind settings panel: {err}"));
            let _ = ready_tx.send(Err(err));
            return;
        }
    };
    let addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(err) => {
            let err = AgentError::Gui(format!("Failed to read listener address: {err}"));
            let _ = ready_tx.send(Err(err));
            return;
        }
    };
    let url = format!("http://{}/", addr);
    if ready_tx.send(Ok(url.clone())).is_err() {
        error!("Failed to notify settings panel startup");
        return;
    }

    if let Err(err) = axum::serve(listener, router).await {
        error!("Settings panel server exited: {err}");
    }
}

async fn settings_page(State(state): State<AppState>) -> Result<Html<String>, StatusCode> {
    let current = load_current_settings(&state.settings_path);
    Ok(Html(render_form(&current, None)))
}

async fn save_settings(
    State(state): State<AppState>,
    Form(input): Form<SettingsFormInput>,
) -> Result<Html<String>, StatusCode> {
    let normalized = StoredSettings {
        coordinator_url: clean_string(input.coordinator_url),
        ollama_port: input.ollama_port,
        heartbeat_interval_secs: input.heartbeat_interval_secs,
    };

    persist_settings(&state.settings_path, &normalized)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Html(render_form(
        &normalized,
        Some("設定を保存しました。エージェントを再起動してください。"),
    )))
}

fn load_current_settings(path: &Path) -> StoredSettings {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<StoredSettings>(&content).ok())
        .unwrap_or_default()
}

fn persist_settings(path: &Path, settings: &StoredSettings) -> Result<(), AgentError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AgentError::Gui(format!("Failed to create settings directory: {err}"))
        })?;
    }

    let mut file = File::create(path)
        .map_err(|err| AgentError::Gui(format!("Failed to open settings file: {err}")))?;
    let content =
        serde_json::to_string_pretty(settings).map_err(|err| AgentError::Gui(err.to_string()))?;
    file.write_all(content.as_bytes())
        .map_err(|err| AgentError::Gui(format!("Failed to write settings file: {err}")))
}

fn settings_file_path() -> Result<PathBuf, AgentError> {
    let home = dirs::home_dir()
        .ok_or_else(|| AgentError::Gui("Failed to resolve home directory".to_string()))?;
    Ok(home.join(".ollama-coordinator").join(SETTINGS_FILE_NAME))
}

fn clean_string(input: Option<String>) -> Option<String> {
    input
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn render_form(settings: &StoredSettings, message: Option<&str>) -> String {
    let coordinator = html_escape(settings.coordinator_url.as_deref().unwrap_or_default());
    let ollama_port = settings
        .ollama_port
        .map(|value| value.to_string())
        .unwrap_or_default();
    let heartbeat = settings
        .heartbeat_interval_secs
        .map(|value| value.to_string())
        .unwrap_or_default();

    let message_block = message
        .map(|text| format!(r#"<div class="notice">{}</div>"#, html_escape(text)))
        .unwrap_or_default();

    format!(
        r#"
<!doctype html>
<html lang="ja">
  <head>
    <meta charset="utf-8" />
    <title>Ollama Coordinator Agent Settings</title>
    <style>
      body {{
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        margin: 40px auto;
        max-width: 640px;
        line-height: 1.6;
      }}
      form {{
        display: flex;
        flex-direction: column;
        gap: 16px;
      }}
      label {{
        display: flex;
        flex-direction: column;
        font-weight: 600;
        gap: 4px;
      }}
      input {{
        padding: 8px 10px;
        border-radius: 6px;
        border: 1px solid #ccc;
        font-size: 16px;
      }}
      button {{
        padding: 12px;
        border: none;
        border-radius: 6px;
        font-size: 16px;
        background-color: #2563eb;
        color: #fff;
        cursor: pointer;
      }}
      button:hover {{
        background-color: #1d4ed8;
      }}
      .notice {{
        padding: 10px 12px;
        border-radius: 6px;
        background-color: #ecfccb;
        color: #3f6212;
        border: 1px solid #bef264;
      }}
      .subtitle {{
        font-size: 14px;
        color: #4b5563;
        margin-bottom: 24px;
      }}
    </style>
  </head>
  <body>
    <h1>Ollama Coordinator Agent 設定</h1>
    <p class="subtitle">{SETTINGS_SUBTITLE}</p>
    {message_block}
    <form method="post">
      <label>
        コーディネーターURL
        <input type="url" name="coordinator_url" value="{coordinator}" placeholder="http://localhost:8080" />
      </label>
      <label>
        Ollamaポート
        <input type="number" name="ollama_port" value="{ollama_port}" placeholder="11434" min="1" max="65535" />
      </label>
      <label>
        ハートビート間隔(秒)
        <input type="number" name="heartbeat_interval_secs" value="{heartbeat}" placeholder="10" min="1" />
      </label>
      <button type="submit">設定を保存</button>
    </form>
  </body>
</html>
"#
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn deserialize_opt_u16<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_number(deserializer)
}

fn deserialize_opt_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_number(deserializer)
}

fn deserialize_optional_number<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    match raw {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                trimmed
                    .parse::<T>()
                    .map(Some)
                    .map_err(|err| de::Error::custom(err.to_string()))
            }
        }
        None => Ok(None),
    }
}
