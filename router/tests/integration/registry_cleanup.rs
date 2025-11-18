//! Integration Test: GPUなしノードの起動時クリーンアップ
//!
//! ストレージに保存されたGPU無しノードが、Coordinator起動時に自動削除されることを確認する。

use once_cell::sync::Lazy;
use or_router::registry::NodeRegistry;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;
use tokio::sync::Mutex;

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn load_fixture(name: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("fixtures")
        .join("agents")
        .join(name);
    let content = fs::read_to_string(path).expect("fixture must exist");
    serde_json::from_str(&content).expect("fixture must be valid JSON array")
}

#[tokio::test]
async fn gpu_less_agents_are_removed_on_startup() {
    let _guard = ENV_LOCK.lock().await;
    let temp = tempdir().expect("create temp dir");

    // 準備: GPU無し・GPU有りのノードを混在させたnodes.jsonを作成
    let mut nodes: Vec<Value> = load_fixture("gpu_missing.json");
    nodes.extend(load_fixture("gpu_valid.json"));

    let data_dir = temp.path();
    let data_file = data_dir.join("nodes.json");
    fs::create_dir_all(data_dir).unwrap();
    fs::write(&data_file, serde_json::to_string_pretty(&nodes).unwrap()).unwrap();

    std::env::set_var("OLLAMA_ROUTER_DATA_DIR", data_dir);

    // Act: ストレージ付きレジストリを初期化
    let registry = NodeRegistry::with_storage()
        .await
        .expect("registry should initialize");

    // Assert: レジストリにはGPUありノードのみ残る
    let remaining = registry.list().await;
    assert_eq!(
        remaining.len(),
        2,
        "only GPU-capable nodes should remain after cleanup"
    );
    assert!(remaining.iter().all(|agent| agent.gpu_available));

    // nodes.jsonが上書きされ、GPU無しノードが削除されていることを確認
    let persisted: Vec<Value> =
        serde_json::from_str(&fs::read_to_string(&data_file).unwrap()).unwrap();
    assert_eq!(persisted.len(), 2);
    assert!(persisted
        .iter()
        .all(|agent| agent["gpu_available"] == Value::Bool(true)));
    assert!(persisted.iter().all(|agent| agent["gpu_devices"]
        .as_array()
        .map(|list| !list.is_empty())
        .unwrap_or(false)));

    std::env::remove_var("OLLAMA_ROUTER_DATA_DIR");
}
