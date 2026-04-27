//! エンドポイントレジストリ
//!
//! エンドポイントの状態をメモリ内で管理し、SQLiteと同期

use crate::db::endpoints as db;
use crate::types::endpoint::{
    Endpoint, EndpointCapability, EndpointModel, EndpointStatus, EndpointType,
};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// 単一エンドポイントが「異常な数」のモデルを申告したと判定する閾値。
/// この値を超えたら `warn!()` ログで運用者に通知する（実態を持たない誤申告の早期検知用）。
const SUSPICIOUS_MODEL_COUNT_THRESHOLD: usize = 50;
use uuid::Uuid;

fn model_lookup_keys(model_id: &str) -> Vec<String> {
    let mut keys = vec![model_id.to_string()];
    if let Some(mapping) = crate::models::mapping::find_mapping(model_id) {
        if !keys.iter().any(|key| key == mapping.canonical) {
            keys.push(mapping.canonical.to_string());
        }
        for alias in mapping.aliases {
            if !keys.iter().any(|key| key == alias.name) {
                keys.push(alias.name.to_string());
            }
        }
    }
    keys
}

fn endpoint_model_lookup_keys(model: &EndpointModel) -> Vec<String> {
    let mut keys = model_lookup_keys(&model.model_id);
    if let Some(canonical) = model.canonical_name.as_deref() {
        for key in model_lookup_keys(canonical) {
            if !keys.iter().any(|existing| existing == &key) {
                keys.push(key);
            }
        }
    }
    keys
}

fn insert_model_mapping(
    model_map: &mut HashMap<String, Vec<Uuid>>,
    model: &EndpointModel,
    endpoint_id: Uuid,
) {
    for key in endpoint_model_lookup_keys(model) {
        let endpoints = model_map.entry(key).or_default();
        if !endpoints.contains(&endpoint_id) {
            endpoints.push(endpoint_id);
        }
    }
}

fn remove_model_mapping(
    model_map: &mut HashMap<String, Vec<Uuid>>,
    model: &EndpointModel,
    endpoint_id: Uuid,
) {
    for key in endpoint_model_lookup_keys(model) {
        let remove_key = if let Some(endpoints) = model_map.get_mut(&key) {
            endpoints.retain(|id| *id != endpoint_id);
            endpoints.is_empty()
        } else {
            false
        };

        if remove_key {
            model_map.remove(&key);
        }
    }
}

/// エンドポイントレジストリ
///
/// エンドポイント情報をメモリにキャッシュし、高速な参照を提供する。
/// 変更はDBと同期される。
#[derive(Clone)]
pub struct EndpointRegistry {
    /// エンドポイントのインメモリキャッシュ
    endpoints: Arc<RwLock<HashMap<Uuid, Endpoint>>>,
    /// モデル→エンドポイントIDのマッピング
    model_to_endpoints: Arc<RwLock<HashMap<String, Vec<Uuid>>>>,
    /// データベースプール
    pool: SqlitePool,
}

impl EndpointRegistry {
    /// SQLiteプールからレジストリを作成し、DBからデータを読み込む
    pub async fn new(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        let registry = Self {
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            model_to_endpoints: Arc::new(RwLock::new(HashMap::new())),
            pool,
        };

        // DBからエンドポイントを読み込み
        registry.load_from_db().await?;

        Ok(registry)
    }

    /// DBからエンドポイントとモデルマッピングを読み込み
    async fn load_from_db(&self) -> Result<(), sqlx::Error> {
        let loaded_endpoints = db::list_endpoints(&self.pool).await?;

        let mut endpoints = self.endpoints.write().await;
        let mut model_map = self.model_to_endpoints.write().await;

        for endpoint in loaded_endpoints {
            let endpoint_id = endpoint.id;

            // モデル一覧を取得
            let models = db::list_endpoint_models(&self.pool, endpoint_id).await?;

            // モデルマッピングを更新
            for model in &models {
                insert_model_mapping(&mut model_map, model, endpoint_id);
            }

            endpoints.insert(endpoint_id, endpoint);
        }

        info!(
            endpoint_count = endpoints.len(),
            model_mappings = model_map.len(),
            "Loaded endpoints from database"
        );

        Ok(())
    }

    /// エンドポイントを取得
    pub async fn get(&self, id: Uuid) -> Option<Endpoint> {
        self.endpoints.read().await.get(&id).cloned()
    }

    /// すべてのエンドポイントを取得
    pub async fn list(&self) -> Vec<Endpoint> {
        self.endpoints.read().await.values().cloned().collect()
    }

    /// オンラインのエンドポイントのみを取得
    pub async fn list_online(&self) -> Vec<Endpoint> {
        self.endpoints
            .read()
            .await
            .values()
            .filter(|e| e.status == EndpointStatus::Online)
            .cloned()
            .collect()
    }

    /// 特定ステータスのエンドポイントを取得
    pub async fn list_by_status(&self, status: EndpointStatus) -> Vec<Endpoint> {
        self.endpoints
            .read()
            .await
            .values()
            .filter(|e| e.status == status)
            .cloned()
            .collect()
    }

    /// 指定した機能を持つオンラインエンドポイントを取得
    ///
    /// 例: ImageGeneration機能を持つエンドポイント → 画像生成リクエストの転送先
    pub async fn list_online_by_capability(&self, capability: EndpointCapability) -> Vec<Endpoint> {
        self.endpoints
            .read()
            .await
            .values()
            .filter(|e| e.status == EndpointStatus::Online && e.has_capability(capability))
            .cloned()
            .collect()
    }

    /// 指定した機能を持つオンラインエンドポイントを補助指標用のレイテンシ順で取得
    ///
    /// SPEC-f8e3a1b7: 推論レイテンシ（EMA α=0.2）でソート。
    /// 複数エンドポイントがある場合、レイテンシが低いものを優先する。
    pub async fn list_online_by_capability_sorted(
        &self,
        capability: EndpointCapability,
    ) -> Vec<Endpoint> {
        let mut endpoints = self.list_online_by_capability(capability).await;
        // SPEC-f8e3a1b7: 推論レイテンシ（inference_latency_ms）でソート
        endpoints.sort_by(|a, b| {
            let a_lat = a.get_inference_latency_for_sort();
            let b_lat = b.get_inference_latency_for_sort();
            a_lat
                .partial_cmp(&b_lat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        endpoints
    }

    /// 指定した機能を持つオンラインエンドポイントが存在するか確認
    pub async fn has_capability_online(&self, capability: EndpointCapability) -> bool {
        self.endpoints
            .read()
            .await
            .values()
            .any(|e| e.status == EndpointStatus::Online && e.has_capability(capability))
    }

    /// モデルIDからエンドポイントを検索
    pub async fn find_by_model(&self, model_id: &str) -> Vec<Endpoint> {
        let model_map = self.model_to_endpoints.read().await;
        let endpoints = self.endpoints.read().await;
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();

        for lookup_key in model_lookup_keys(model_id) {
            if let Some(ids) = model_map.get(&lookup_key) {
                for id in ids {
                    if !seen.insert(*id) {
                        continue;
                    }
                    if let Some(endpoint) = endpoints.get(id) {
                        if endpoint.status == EndpointStatus::Online {
                            resolved.push(endpoint.clone());
                        }
                    }
                }
            }
        }

        resolved
    }

    /// 補助指標用にレイテンシ順でエンドポイントをソート（低レイテンシ優先）
    ///
    /// SPEC-f8e3a1b7: 推論レイテンシ（EMA α=0.2）を使用してソート。
    /// レイテンシが同じ場合はラウンドロビンでタイブレーク。
    pub async fn find_by_model_sorted_by_latency(&self, model_id: &str) -> Vec<Endpoint> {
        let mut endpoints = self.find_by_model(model_id).await;
        // SPEC-f8e3a1b7: 推論レイテンシ（inference_latency_ms）でソート
        endpoints.sort_by(|a, b| {
            let a_lat = a.get_inference_latency_for_sort();
            let b_lat = b.get_inference_latency_for_sort();
            a_lat
                .partial_cmp(&b_lat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        endpoints
    }

    /// エンドポイントを追加（DBとキャッシュ両方に保存）
    pub async fn add(&self, endpoint: Endpoint) -> Result<(), sqlx::Error> {
        // DBに保存
        db::create_endpoint(&self.pool, &endpoint).await?;

        // キャッシュに追加
        self.endpoints.write().await.insert(endpoint.id, endpoint);

        Ok(())
    }

    /// エンドポイントをキャッシュのみに追加（DBは更新しない）
    ///
    /// 外部でDB保存が完了した後にキャッシュを同期するために使用する。
    pub async fn add_to_cache(&self, endpoint: Endpoint) {
        self.endpoints.write().await.insert(endpoint.id, endpoint);
    }

    /// エンドポイントを更新（DBとキャッシュ両方）
    pub async fn update(&self, endpoint: Endpoint) -> Result<bool, sqlx::Error> {
        // DBを更新
        let updated = db::update_endpoint(&self.pool, &endpoint).await?;

        if updated {
            // キャッシュを更新
            self.endpoints.write().await.insert(endpoint.id, endpoint);
        }

        Ok(updated)
    }

    /// エンドポイントのステータスを更新
    pub async fn update_status(
        &self,
        id: Uuid,
        status: EndpointStatus,
        latency_ms: Option<u32>,
        error: Option<&str>,
    ) -> Result<bool, sqlx::Error> {
        // DBを更新
        let updated = db::update_endpoint_status(&self.pool, id, status, latency_ms, error).await?;

        if updated {
            // キャッシュを更新
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.status = status;
                if let Some(v) = latency_ms {
                    endpoint.latency_ms = Some(v);
                }
                // DBと同様に、last_error は成功時にクリアし、error_count は status=error のときのみ加算する。
                endpoint.last_error = error.map(String::from);
                endpoint.error_count = if status == EndpointStatus::Error {
                    endpoint.error_count.saturating_add(1)
                } else {
                    0
                };
                endpoint.last_seen = Some(chrono::Utc::now());
            }
        }

        Ok(updated)
    }

    /// エンドポイントのGPU情報を更新（キャッシュのみ、DBには保存しない）
    ///
    /// `/api/health`から取得したGPU情報をキャッシュに反映する。
    /// GPU情報は頻繁に変化するため、DBには保存せずメモリ上でのみ管理する。
    pub async fn update_gpu_info(
        &self,
        id: Uuid,
        gpu_device_count: Option<u32>,
        gpu_total_memory_bytes: Option<u64>,
        gpu_used_memory_bytes: Option<u64>,
        gpu_capability_score: Option<f32>,
        active_requests: Option<u32>,
    ) -> bool {
        let mut endpoints = self.endpoints.write().await;
        if let Some(endpoint) = endpoints.get_mut(&id) {
            endpoint.gpu_device_count = gpu_device_count;
            endpoint.gpu_total_memory_bytes = gpu_total_memory_bytes;
            endpoint.gpu_used_memory_bytes = gpu_used_memory_bytes;
            endpoint.gpu_capability_score = gpu_capability_score;
            endpoint.active_requests = active_requests;
            true
        } else {
            false
        }
    }

    /// エンドポイントのタイプを更新（DBとキャッシュ両方）（SPEC-e8e9326e）
    ///
    /// ヘルスチェック時のoffline→online遷移時に再検出して更新する。
    pub async fn update_endpoint_type(
        &self,
        id: Uuid,
        endpoint_type: EndpointType,
    ) -> Result<bool, sqlx::Error> {
        // DBを更新
        let updated = db::update_endpoint_type(&self.pool, id, endpoint_type).await?;

        if updated {
            // キャッシュを更新
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.endpoint_type = endpoint_type;
            }
        }

        Ok(updated)
    }

    /// エンドポイントの推論レイテンシを更新（DBとキャッシュ両方）（SPEC-f8e3a1b7）
    ///
    /// 推論リクエスト完了時に呼び出し、EMA（α=0.2）で平均レイテンシを計算する。
    /// オフライン時は`reset_inference_latency`を呼び出す。
    pub async fn update_inference_latency(
        &self,
        id: Uuid,
        new_latency_ms: f64,
    ) -> Result<bool, sqlx::Error> {
        // キャッシュを更新（EMA計算はEndpoint内で行う）
        let inference_latency_ms = {
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.update_inference_latency(new_latency_ms);
                endpoint.inference_latency_ms
            } else {
                return Ok(false);
            }
        };

        // DBを更新
        db::update_inference_latency(&self.pool, id, inference_latency_ms).await
    }

    /// エンドポイントの推論レイテンシをリセット（オフライン時）（SPEC-f8e3a1b7）
    ///
    /// エンドポイントがオフラインになったときに呼び出し、レイテンシをINFINITYに設定。
    pub async fn reset_inference_latency(&self, id: Uuid) -> Result<bool, sqlx::Error> {
        // キャッシュを更新
        {
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.reset_inference_latency();
            }
        }

        // DBを更新（INFINITYを保存）
        db::update_inference_latency(&self.pool, id, Some(f64::INFINITY)).await
    }

    /// エンドポイントのデバイス情報を更新（DBとキャッシュ両方）（SPEC-f8e3a1b7）
    ///
    /// /api/system APIから取得したデバイス情報を保存する。
    pub async fn update_device_info(
        &self,
        id: Uuid,
        device_info: Option<crate::types::endpoint::DeviceInfo>,
    ) -> Result<bool, sqlx::Error> {
        // キャッシュを更新
        {
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.device_info = device_info.clone();
            } else {
                return Ok(false);
            }
        }

        // DBを更新
        db::update_device_info(&self.pool, id, device_info.as_ref()).await
    }

    /// エンドポイントのリクエストカウンタをインクリメント（DBとキャッシュ両方）
    pub async fn increment_request_counters(
        &self,
        id: Uuid,
        success: bool,
    ) -> Result<bool, sqlx::Error> {
        let updated = db::increment_request_counters(&self.pool, id, success).await?;

        if updated {
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.total_requests += 1;
                if success {
                    endpoint.successful_requests += 1;
                } else {
                    endpoint.failed_requests += 1;
                }
            }
        }

        Ok(updated)
    }

    /// エンドポイントを削除（DBとキャッシュ両方）
    pub async fn remove(&self, id: Uuid) -> Result<bool, sqlx::Error> {
        // モデルマッピングから削除
        {
            let mut model_map = self.model_to_endpoints.write().await;
            for endpoints in model_map.values_mut() {
                endpoints.retain(|eid| *eid != id);
            }
            // 空になったエントリを削除
            model_map.retain(|_, v| !v.is_empty());
        }

        // DBから削除
        let deleted = db::delete_endpoint(&self.pool, id).await?;

        if deleted {
            // キャッシュから削除
            self.endpoints.write().await.remove(&id);
        }

        Ok(deleted)
    }

    /// モデルを追加
    pub async fn add_model(&self, model: &EndpointModel) -> Result<(), sqlx::Error> {
        // DBに保存
        db::add_endpoint_model(&self.pool, model).await?;

        // モデルマッピングを更新
        let mut model_map = self.model_to_endpoints.write().await;
        insert_model_mapping(&mut model_map, model, model.endpoint_id);

        Ok(())
    }

    /// エンドポイントのモデルを同期（追加/削除）
    pub async fn sync_models(
        &self,
        endpoint_id: Uuid,
        models: Vec<EndpointModel>,
    ) -> Result<SyncResult, sqlx::Error> {
        // 既存モデルを取得
        let existing = db::list_endpoint_models(&self.pool, endpoint_id).await?;
        let existing_ids: std::collections::HashSet<_> =
            existing.iter().map(|m| &m.model_id).collect();

        let new_ids: std::collections::HashSet<_> = models.iter().map(|m| &m.model_id).collect();

        // 追加されたモデル
        let added: Vec<_> = models
            .iter()
            .filter(|m| !existing_ids.contains(&m.model_id))
            .cloned()
            .collect();

        // 削除されたモデル
        let removed: Vec<_> = existing
            .iter()
            .filter(|m| !new_ids.contains(&m.model_id))
            .cloned()
            .collect();

        // DBを更新
        for model in &added {
            db::add_endpoint_model(&self.pool, model).await?;
        }

        for model in &removed {
            db::delete_endpoint_model(&self.pool, endpoint_id, &model.model_id).await?;
        }

        // モデルマッピングを更新
        {
            let mut model_map = self.model_to_endpoints.write().await;

            // 追加
            for model in &added {
                insert_model_mapping(&mut model_map, model, endpoint_id);
            }

            // 削除
            for model in &removed {
                remove_model_mapping(&mut model_map, model, endpoint_id);
            }
        }

        debug!(
            endpoint_id = %endpoint_id,
            added = added.len(),
            removed = removed.len(),
            total = models.len(),
            "Synced endpoint models"
        );

        // 単一エンドポイントが過剰なモデル数を申告した場合に警告（誤申告/設定ミスの早期検知）。
        // 例: カタログ集約サーバが実体なしの全モデルを `/v1/models` で返してしまうケース。
        if models.len() > SUSPICIOUS_MODEL_COUNT_THRESHOLD {
            warn!(
                endpoint_id = %endpoint_id,
                model_count = models.len(),
                threshold = SUSPICIOUS_MODEL_COUNT_THRESHOLD,
                "Endpoint reported suspiciously many models. Verify the endpoint is not aggregating models it cannot serve (see CLAUDE.md C-1/C-2 notes)"
            );
        }

        Ok(SyncResult {
            added: added.len(),
            removed: removed.len(),
            total: models.len(),
        })
    }

    /// エンドポイントのモデル一覧を取得
    pub async fn list_models(&self, endpoint_id: Uuid) -> Result<Vec<EndpointModel>, sqlx::Error> {
        db::list_endpoint_models(&self.pool, endpoint_id).await
    }

    /// モデルマッピングを指定エンドポイント分だけ再構築
    pub async fn refresh_model_mappings(&self, endpoint_id: Uuid) -> Result<(), sqlx::Error> {
        let models = db::list_endpoint_models(&self.pool, endpoint_id).await?;

        let mut model_map = self.model_to_endpoints.write().await;

        // 既存マッピングから当該エンドポイントを除外
        model_map.retain(|_, endpoints| {
            endpoints.retain(|id| *id != endpoint_id);
            !endpoints.is_empty()
        });

        // 取得したモデルでマッピングを再構築
        for model in models {
            insert_model_mapping(&mut model_map, &model, endpoint_id);
        }

        Ok(())
    }

    /// 全モデルIDの一覧を取得
    pub async fn list_all_model_ids(&self) -> Vec<String> {
        self.model_to_endpoints
            .read()
            .await
            .keys()
            .cloned()
            .collect()
    }

    /// キャッシュをDBから再読み込み
    pub async fn reload(&self) -> Result<(), sqlx::Error> {
        // キャッシュをクリア
        {
            self.endpoints.write().await.clear();
            self.model_to_endpoints.write().await.clear();
        }

        // DBから再読み込み
        self.load_from_db().await
    }

    /// エンドポイント数を取得
    pub async fn count(&self) -> usize {
        self.endpoints.read().await.len()
    }

    /// DBプールへの参照を取得
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

/// モデル同期結果
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// 追加されたモデル数
    pub added: usize,
    /// 削除されたモデル数
    pub removed: usize,
    /// 同期後のモデル総数
    pub total: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_utils::TEST_LOCK;
    use crate::types::endpoint::{EndpointCapability, SupportedAPI};

    async fn setup_test_db() -> SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    #[tokio::test]
    async fn test_registry_basic_operations() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        // エンドポイントを追加
        let endpoint = Endpoint::new(
            "Test Endpoint".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );
        let endpoint_id = endpoint.id;

        registry.add(endpoint).await.unwrap();

        // 取得確認
        let retrieved = registry.get(endpoint_id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Test Endpoint");

        // 一覧確認
        let all = registry.list().await;
        assert_eq!(all.len(), 1);

        // 削除
        let deleted = registry.remove(endpoint_id).await.unwrap();
        assert!(deleted);

        // 削除後は取得できない
        assert!(registry.get(endpoint_id).await.is_none());
    }

    #[tokio::test]
    async fn test_registry_model_mapping() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        // エンドポイントを追加
        let mut endpoint = Endpoint::new(
            "Test".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;

        registry.add(endpoint).await.unwrap();

        // モデルを追加
        let model = EndpointModel {
            endpoint_id,
            model_id: "llama3:8b".to_string(),
            capabilities: Some(vec!["chat".to_string()]),
            max_tokens: None,
            last_checked: Some(chrono::Utc::now()),
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: None,
        };

        registry.add_model(&model).await.unwrap();

        // モデルIDで検索
        let found = registry.find_by_model("llama3:8b").await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, endpoint_id);

        // 存在しないモデル
        let not_found = registry.find_by_model("nonexistent").await;
        assert!(not_found.is_empty());
    }

    #[tokio::test]
    async fn test_registry_status_update() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        // エンドポイントを追加
        let endpoint = Endpoint::new(
            "Test".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );
        let endpoint_id = endpoint.id;

        registry.add(endpoint).await.unwrap();

        // ステータスを更新
        registry
            .update_status(endpoint_id, EndpointStatus::Online, Some(50), None)
            .await
            .unwrap();

        // 確認
        let updated = registry.get(endpoint_id).await.unwrap();
        assert_eq!(updated.status, EndpointStatus::Online);
        assert_eq!(updated.latency_ms, Some(50));

        // オンラインエンドポイントのみ取得
        let online = registry.list_online().await;
        assert_eq!(online.len(), 1);
    }

    #[tokio::test]
    async fn test_find_by_canonical_name_for_alias_backed_model() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut endpoint = Endpoint::new(
            "Alias-backed".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Ollama,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        registry.add(endpoint).await.unwrap();

        registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: "gpt-oss:20b".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: Some("openai/gpt-oss-20b".to_string()),
            })
            .await
            .unwrap();

        let found = registry.find_by_model("openai/gpt-oss-20b").await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, endpoint_id);
    }

    /// T005 [US1]: increment_request_counters がDBとキャッシュの両方の
    /// リクエストカウンタを正しくインクリメントすることを検証
    #[tokio::test]
    async fn test_increment_request_counters_syncs_cache() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        // (1) エンドポイント登録（カウンタ=0）
        let endpoint = Endpoint::new(
            "Counter Sync".to_string(),
            "http://localhost:9300".to_string(),
            EndpointType::Xllm,
        );
        let endpoint_id = endpoint.id;
        registry.add(endpoint).await.unwrap();

        let ep = registry.get(endpoint_id).await.unwrap();
        assert_eq!(ep.total_requests, 0);
        assert_eq!(ep.successful_requests, 0);
        assert_eq!(ep.failed_requests, 0);

        // (2) success=true でインクリメント
        registry
            .increment_request_counters(endpoint_id, true)
            .await
            .unwrap();

        // (3) キャッシュの total_requests=1, successful_requests=1 を確認
        let ep = registry.get(endpoint_id).await.unwrap();
        assert_eq!(ep.total_requests, 1);
        assert_eq!(ep.successful_requests, 1);
        assert_eq!(ep.failed_requests, 0);

        // (4) success=false でインクリメント
        registry
            .increment_request_counters(endpoint_id, false)
            .await
            .unwrap();

        // (5) キャッシュの total_requests=2, failed_requests=1 を確認
        let ep = registry.get(endpoint_id).await.unwrap();
        assert_eq!(ep.total_requests, 2);
        assert_eq!(ep.successful_requests, 1);
        assert_eq!(ep.failed_requests, 1);
    }

    /// T002 [US2]: update_status で latency_ms=None を渡した場合に
    /// キャッシュ内のレイテンシ値が保持されることを検証
    #[tokio::test]
    async fn test_update_status_preserves_latency_in_cache() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        // (1) エンドポイント追加
        let endpoint = Endpoint::new(
            "Cache Latency".to_string(),
            "http://localhost:9200".to_string(),
            EndpointType::Xllm,
        );
        let endpoint_id = endpoint.id;
        registry.add(endpoint).await.unwrap();

        // (2) latency_ms=Some(120) でオンラインに更新
        registry
            .update_status(endpoint_id, EndpointStatus::Online, Some(120), None)
            .await
            .unwrap();
        let ep = registry.get(endpoint_id).await.unwrap();
        assert_eq!(ep.latency_ms, Some(120));

        // (3) latency_ms=None でオフラインに遷移
        registry
            .update_status(
                endpoint_id,
                EndpointStatus::Offline,
                None,
                Some("health check failed"),
            )
            .await
            .unwrap();

        // (4) キャッシュから読み取り、latency_ms が Some(120) のまま保持されていることを確認
        let ep = registry.get(endpoint_id).await.unwrap();
        assert_eq!(
            ep.latency_ms,
            Some(120),
            "cache latency_ms should be preserved when None is passed"
        );
        assert_eq!(ep.status, EndpointStatus::Offline);
    }

    #[tokio::test]
    async fn test_registry_capability_filter() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        // チャット機能のみのエンドポイント
        let mut ep_chat = Endpoint::new(
            "Chat Only".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );
        ep_chat.status = EndpointStatus::Online;
        ep_chat.capabilities = vec![EndpointCapability::ChatCompletion];
        registry.add(ep_chat).await.unwrap();

        // 画像生成機能を持つエンドポイント
        let mut ep_image = Endpoint::new(
            "Image Gen".to_string(),
            "http://localhost:7860".to_string(),
            EndpointType::Xllm,
        );
        ep_image.status = EndpointStatus::Online;
        ep_image.capabilities = vec![
            EndpointCapability::ChatCompletion,
            EndpointCapability::ImageGeneration,
        ];
        ep_image.latency_ms = Some(100);
        registry.add(ep_image).await.unwrap();

        // 音声認識機能を持つエンドポイント（オフライン）
        let mut ep_audio = Endpoint::new(
            "ASR".to_string(),
            "http://localhost:8080".to_string(),
            EndpointType::Xllm,
        );
        ep_audio.status = EndpointStatus::Offline;
        ep_audio.capabilities = vec![EndpointCapability::AudioTranscription];
        registry.add(ep_audio).await.unwrap();

        // チャット機能で検索 → 2件
        let chat_endpoints = registry
            .list_online_by_capability(EndpointCapability::ChatCompletion)
            .await;
        assert_eq!(chat_endpoints.len(), 2);

        // 画像生成機能で検索 → 1件
        let image_endpoints = registry
            .list_online_by_capability(EndpointCapability::ImageGeneration)
            .await;
        assert_eq!(image_endpoints.len(), 1);
        assert_eq!(image_endpoints[0].name, "Image Gen");

        // 音声認識機能で検索 → 0件（オフラインなので）
        let audio_endpoints = registry
            .list_online_by_capability(EndpointCapability::AudioTranscription)
            .await;
        assert!(audio_endpoints.is_empty());

        // 機能存在チェック
        assert!(
            registry
                .has_capability_online(EndpointCapability::ImageGeneration)
                .await
        );
        assert!(
            !registry
                .has_capability_online(EndpointCapability::AudioTranscription)
                .await
        );
        assert!(
            !registry
                .has_capability_online(EndpointCapability::AudioSpeech)
                .await
        );
    }

    // ===== 追加テスト: get / list / count =====

    #[tokio::test]
    async fn test_get_nonexistent_returns_none() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let result = registry.get(Uuid::new_v4()).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_empty_registry() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let all = registry.list().await;
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn test_count_empty() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_count_after_add() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "Count Test".to_string(),
            "http://localhost:9000".to_string(),
            EndpointType::Xllm,
        );
        registry.add(ep).await.unwrap();
        assert_eq!(registry.count().await, 1);
    }

    // ===== list_online / list_by_status テスト =====

    #[tokio::test]
    async fn test_list_online_excludes_offline() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut online_ep = Endpoint::new(
            "Online".to_string(),
            "http://localhost:9001".to_string(),
            EndpointType::Xllm,
        );
        online_ep.status = EndpointStatus::Online;
        registry.add(online_ep).await.unwrap();

        let mut offline_ep = Endpoint::new(
            "Offline".to_string(),
            "http://localhost:9002".to_string(),
            EndpointType::Xllm,
        );
        offline_ep.status = EndpointStatus::Offline;
        registry.add(offline_ep).await.unwrap();

        let online = registry.list_online().await;
        assert_eq!(online.len(), 1);
        assert_eq!(online[0].name, "Online");
    }

    #[tokio::test]
    async fn test_list_by_status_pending() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let pending_ep = Endpoint::new(
            "Pending".to_string(),
            "http://localhost:9003".to_string(),
            EndpointType::Xllm,
        );
        // デフォルトステータスはPending
        registry.add(pending_ep).await.unwrap();

        let mut online_ep = Endpoint::new(
            "Online".to_string(),
            "http://localhost:9004".to_string(),
            EndpointType::Xllm,
        );
        online_ep.status = EndpointStatus::Online;
        registry.add(online_ep).await.unwrap();

        let pending = registry.list_by_status(EndpointStatus::Pending).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].name, "Pending");
    }

    #[tokio::test]
    async fn test_list_by_status_error() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut error_ep = Endpoint::new(
            "ErrorEP".to_string(),
            "http://localhost:9005".to_string(),
            EndpointType::Xllm,
        );
        error_ep.status = EndpointStatus::Error;
        registry.add(error_ep).await.unwrap();

        let errors = registry.list_by_status(EndpointStatus::Error).await;
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].name, "ErrorEP");
    }

    // ===== add_to_cache テスト =====

    #[tokio::test]
    async fn test_add_to_cache_only_updates_memory() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "CacheOnly".to_string(),
            "http://localhost:9006".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add_to_cache(ep).await;

        // キャッシュにある
        let cached = registry.get(ep_id).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().name, "CacheOnly");

        // count確認
        assert_eq!(registry.count().await, 1);
    }

    // ===== update テスト =====

    #[tokio::test]
    async fn test_update_endpoint() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep = Endpoint::new(
            "Original".to_string(),
            "http://localhost:9007".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep.clone()).await.unwrap();

        ep.name = "Updated".to_string();
        let updated = registry.update(ep).await.unwrap();
        assert!(updated);

        let retrieved = registry.get(ep_id).await.unwrap();
        assert_eq!(retrieved.name, "Updated");
    }

    // ===== update_status テスト =====

    #[tokio::test]
    async fn test_update_status_with_error() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "StatusTest".to_string(),
            "http://localhost:9008".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        registry
            .update_status(
                ep_id,
                EndpointStatus::Error,
                None,
                Some("connection refused"),
            )
            .await
            .unwrap();

        let updated = registry.get(ep_id).await.unwrap();
        assert_eq!(updated.status, EndpointStatus::Error);
        assert_eq!(updated.last_error.as_deref(), Some("connection refused"));
        assert_eq!(updated.error_count, 1);
    }

    #[tokio::test]
    async fn test_update_status_online_clears_error() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "ClearError".to_string(),
            "http://localhost:9009".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        // エラー状態にする
        registry
            .update_status(ep_id, EndpointStatus::Error, None, Some("some error"))
            .await
            .unwrap();

        // オンラインに戻す
        registry
            .update_status(ep_id, EndpointStatus::Online, Some(50), None)
            .await
            .unwrap();

        let updated = registry.get(ep_id).await.unwrap();
        assert_eq!(updated.status, EndpointStatus::Online);
        assert!(updated.last_error.is_none());
        assert_eq!(updated.error_count, 0);
    }

    // ===== update_gpu_info テスト =====

    #[tokio::test]
    async fn test_update_gpu_info_existing() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "GPUTest".to_string(),
            "http://localhost:9010".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        let result = registry
            .update_gpu_info(
                ep_id,
                Some(1),
                Some(16_000_000_000),
                Some(8_000_000_000),
                Some(95.0),
                Some(3),
            )
            .await;
        assert!(result);

        let updated = registry.get(ep_id).await.unwrap();
        assert_eq!(updated.gpu_device_count, Some(1));
        assert_eq!(updated.gpu_total_memory_bytes, Some(16_000_000_000));
        assert_eq!(updated.gpu_used_memory_bytes, Some(8_000_000_000));
        assert_eq!(updated.active_requests, Some(3));
    }

    #[tokio::test]
    async fn test_update_gpu_info_nonexistent() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let result = registry
            .update_gpu_info(Uuid::new_v4(), None, None, None, None, None)
            .await;
        assert!(!result);
    }

    // ===== update_endpoint_type テスト =====

    #[tokio::test]
    async fn test_update_endpoint_type() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "TypeTest".to_string(),
            "http://localhost:9011".to_string(),
            EndpointType::OpenaiCompatible,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        let updated = registry
            .update_endpoint_type(ep_id, EndpointType::Xllm)
            .await
            .unwrap();
        assert!(updated);

        let retrieved = registry.get(ep_id).await.unwrap();
        assert_eq!(retrieved.endpoint_type, EndpointType::Xllm);
    }

    // ===== update_inference_latency / reset_inference_latency テスト =====

    #[tokio::test]
    async fn test_update_inference_latency() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "LatencyTest".to_string(),
            "http://localhost:9012".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        registry
            .update_inference_latency(ep_id, 100.0)
            .await
            .unwrap();

        let updated = registry.get(ep_id).await.unwrap();
        assert!(updated.inference_latency_ms.is_some());
        let latency = updated.inference_latency_ms.unwrap();
        assert!(
            (latency - 100.0).abs() < 0.01,
            "expected 100.0, got {latency}"
        );
    }

    #[tokio::test]
    async fn test_update_inference_latency_nonexistent() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let result = registry
            .update_inference_latency(Uuid::new_v4(), 100.0)
            .await
            .unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn test_reset_inference_latency() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "ResetLatency".to_string(),
            "http://localhost:9013".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        // まずレイテンシを設定
        registry
            .update_inference_latency(ep_id, 50.0)
            .await
            .unwrap();

        // リセット
        registry.reset_inference_latency(ep_id).await.unwrap();

        let updated = registry.get(ep_id).await.unwrap();
        let latency = updated.inference_latency_ms.unwrap();
        assert!(latency.is_infinite(), "expected INFINITY, got {latency}");
    }

    // ===== remove テスト =====

    #[tokio::test]
    async fn test_remove_nonexistent() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let result = registry.remove(Uuid::new_v4()).await.unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn test_remove_cleans_model_mapping() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep = Endpoint::new(
            "RemoveModel".to_string(),
            "http://localhost:9014".to_string(),
            EndpointType::Xllm,
        );
        ep.status = EndpointStatus::Online;
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        let model = EndpointModel {
            endpoint_id: ep_id,
            model_id: "cleanup-model".to_string(),
            capabilities: None,
            max_tokens: None,
            last_checked: None,
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: None,
        };
        registry.add_model(&model).await.unwrap();

        // モデルで見つかることを確認
        let found = registry.find_by_model("cleanup-model").await;
        assert_eq!(found.len(), 1);

        // エンドポイント削除
        registry.remove(ep_id).await.unwrap();

        // モデルマッピングもクリアされている
        let found = registry.find_by_model("cleanup-model").await;
        assert!(found.is_empty());
    }

    // ===== sync_models テスト =====

    #[tokio::test]
    async fn test_sync_models_add_new() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep = Endpoint::new(
            "SyncTest".to_string(),
            "http://localhost:9015".to_string(),
            EndpointType::Xllm,
        );
        ep.status = EndpointStatus::Online;
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        let models = vec![
            EndpointModel {
                endpoint_id: ep_id,
                model_id: "model-a".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            },
            EndpointModel {
                endpoint_id: ep_id,
                model_id: "model-b".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            },
        ];

        let result = registry.sync_models(ep_id, models).await.unwrap();
        assert_eq!(result.added, 2);
        assert_eq!(result.removed, 0);
        assert_eq!(result.total, 2);
    }

    #[tokio::test]
    async fn test_sync_models_remove_old() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep = Endpoint::new(
            "SyncRemove".to_string(),
            "http://localhost:9016".to_string(),
            EndpointType::Xllm,
        );
        ep.status = EndpointStatus::Online;
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        // 初期モデル追加
        registry
            .add_model(&EndpointModel {
                endpoint_id: ep_id,
                model_id: "old-model".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .unwrap();

        // 新しいモデルリストで同期（old-modelは含まない）
        let new_models = vec![EndpointModel {
            endpoint_id: ep_id,
            model_id: "new-model".to_string(),
            capabilities: None,
            max_tokens: None,
            last_checked: None,
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: None,
        }];

        let result = registry.sync_models(ep_id, new_models).await.unwrap();
        assert_eq!(result.added, 1);
        assert_eq!(result.removed, 1);
        assert_eq!(result.total, 1);

        // old-modelで検索してもヒットしない
        let found = registry.find_by_model("old-model").await;
        assert!(found.is_empty());
    }

    // ===== list_models テスト =====

    #[tokio::test]
    async fn test_list_models_empty() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "NoModels".to_string(),
            "http://localhost:9017".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        let models = registry.list_models(ep_id).await.unwrap();
        assert!(models.is_empty());
    }

    #[tokio::test]
    async fn test_list_models_with_data() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "WithModels".to_string(),
            "http://localhost:9018".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        registry
            .add_model(&EndpointModel {
                endpoint_id: ep_id,
                model_id: "listed-model".to_string(),
                capabilities: Some(vec!["chat".to_string()]),
                max_tokens: Some(4096),
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .unwrap();

        let models = registry.list_models(ep_id).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "listed-model");
    }

    // ===== list_all_model_ids テスト =====

    #[tokio::test]
    async fn test_list_all_model_ids_empty() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let ids = registry.list_all_model_ids().await;
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn test_list_all_model_ids_with_data() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep = Endpoint::new(
            "ModelIds".to_string(),
            "http://localhost:9019".to_string(),
            EndpointType::Xllm,
        );
        ep.status = EndpointStatus::Online;
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        registry
            .add_model(&EndpointModel {
                endpoint_id: ep_id,
                model_id: "id-model-a".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .unwrap();

        registry
            .add_model(&EndpointModel {
                endpoint_id: ep_id,
                model_id: "id-model-b".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .unwrap();

        let ids = registry.list_all_model_ids().await;
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"id-model-a".to_string()));
        assert!(ids.contains(&"id-model-b".to_string()));
    }

    // ===== find_by_model テスト =====

    #[tokio::test]
    async fn test_find_by_model_excludes_offline() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep = Endpoint::new(
            "OfflineModel".to_string(),
            "http://localhost:9020".to_string(),
            EndpointType::Xllm,
        );
        ep.status = EndpointStatus::Offline;
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        registry
            .add_model(&EndpointModel {
                endpoint_id: ep_id,
                model_id: "offline-model".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .unwrap();

        let found = registry.find_by_model("offline-model").await;
        assert!(found.is_empty(), "offline endpoints should be excluded");
    }

    #[tokio::test]
    async fn test_find_by_model_matches_canonical_and_aliases() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep = Endpoint::new(
            "AliasBacked".to_string(),
            "http://localhost:9021".to_string(),
            EndpointType::LmStudio,
        );
        ep.status = EndpointStatus::Online;
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        registry
            .add_model(&EndpointModel {
                endpoint_id: ep_id,
                model_id: "qwen/qwen3-coder-30b".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: Some("Qwen/qwen3-coder-30b".to_string()),
            })
            .await
            .unwrap();

        let canonical = registry.find_by_model("Qwen/qwen3-coder-30b").await;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].id, ep_id);

        let alias = registry.find_by_model("qwen/qwen3-coder-30b").await;
        assert_eq!(alias.len(), 1);
        assert_eq!(alias[0].id, ep_id);
    }

    // ===== find_by_model_sorted_by_latency テスト =====

    #[tokio::test]
    async fn test_find_by_model_sorted_by_latency() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep_slow = Endpoint::new(
            "Slow".to_string(),
            "http://localhost:9021".to_string(),
            EndpointType::Xllm,
        );
        ep_slow.status = EndpointStatus::Online;
        ep_slow.inference_latency_ms = Some(500.0);
        let slow_id = ep_slow.id;
        registry.add(ep_slow).await.unwrap();

        let mut ep_fast = Endpoint::new(
            "Fast".to_string(),
            "http://localhost:9022".to_string(),
            EndpointType::Xllm,
        );
        ep_fast.status = EndpointStatus::Online;
        ep_fast.inference_latency_ms = Some(50.0);
        let fast_id = ep_fast.id;
        registry.add(ep_fast).await.unwrap();

        for ep_id in [slow_id, fast_id] {
            registry
                .add_model(&EndpointModel {
                    endpoint_id: ep_id,
                    model_id: "sort-test-model".to_string(),
                    capabilities: None,
                    max_tokens: None,
                    last_checked: None,
                    supported_apis: vec![SupportedAPI::ChatCompletions],
                    canonical_name: None,
                })
                .await
                .unwrap();
        }

        let sorted = registry
            .find_by_model_sorted_by_latency("sort-test-model")
            .await;
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].id, fast_id, "faster endpoint should be first");
        assert_eq!(sorted[1].id, slow_id, "slower endpoint should be second");
    }

    // ===== list_online_by_capability_sorted テスト =====

    #[tokio::test]
    async fn test_list_online_by_capability_sorted() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep_high = Endpoint::new(
            "HighLatency".to_string(),
            "http://localhost:9023".to_string(),
            EndpointType::Xllm,
        );
        ep_high.status = EndpointStatus::Online;
        ep_high.capabilities = vec![EndpointCapability::ChatCompletion];
        ep_high.inference_latency_ms = Some(1000.0);
        let high_id = ep_high.id;
        registry.add(ep_high).await.unwrap();

        let mut ep_low = Endpoint::new(
            "LowLatency".to_string(),
            "http://localhost:9024".to_string(),
            EndpointType::Xllm,
        );
        ep_low.status = EndpointStatus::Online;
        ep_low.capabilities = vec![EndpointCapability::ChatCompletion];
        ep_low.inference_latency_ms = Some(10.0);
        let low_id = ep_low.id;
        registry.add(ep_low).await.unwrap();

        let sorted = registry
            .list_online_by_capability_sorted(EndpointCapability::ChatCompletion)
            .await;
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].id, low_id);
        assert_eq!(sorted[1].id, high_id);
    }

    // ===== reload テスト =====

    #[tokio::test]
    async fn test_reload_refreshes_cache() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "ReloadTest".to_string(),
            "http://localhost:9025".to_string(),
            EndpointType::Xllm,
        );
        registry.add(ep).await.unwrap();
        assert_eq!(registry.count().await, 1);

        // reload後もデータ保持
        registry.reload().await.unwrap();
        assert_eq!(registry.count().await, 1);
    }

    // ===== pool テスト =====

    #[tokio::test]
    async fn test_pool_returns_reference() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        // pool()がSqlitePoolへの参照を返すことを確認
        let _pool_ref = registry.pool();
    }

    // ===== refresh_model_mappings テスト =====

    #[tokio::test]
    async fn test_refresh_model_mappings() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep = Endpoint::new(
            "RefreshMap".to_string(),
            "http://localhost:9026".to_string(),
            EndpointType::Xllm,
        );
        ep.status = EndpointStatus::Online;
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        registry
            .add_model(&EndpointModel {
                endpoint_id: ep_id,
                model_id: "refresh-model".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .unwrap();

        // リフレッシュ
        registry.refresh_model_mappings(ep_id).await.unwrap();

        // モデルマッピングが正しく再構築されている
        let found = registry.find_by_model("refresh-model").await;
        assert_eq!(found.len(), 1);
    }

    /// C-defensive: 単一エンドポイントが 50 件超を申告しても sync_models が成功すること
    /// （warn ログが出力される動作は tracing 出力検証の範囲外。本テストは regression 用）
    #[tokio::test]
    async fn test_sync_models_handles_suspiciously_large_model_list() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mut ep = Endpoint::new(
            "ManyModelsEndpoint".to_string(),
            "http://localhost:9027".to_string(),
            EndpointType::Xllm,
        );
        ep.status = EndpointStatus::Online;
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        // 閾値（50）を確実に超える 60 モデルを申告
        let models: Vec<EndpointModel> = (0..60)
            .map(|i| EndpointModel {
                endpoint_id: ep_id,
                model_id: format!("test-model-{:03}", i),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .collect();

        let result = registry.sync_models(ep_id, models).await.unwrap();
        assert_eq!(result.added, 60);
        assert_eq!(result.removed, 0);
        assert_eq!(result.total, 60);
    }

    // ===== SyncResult テスト =====

    #[test]
    fn test_sync_result_debug() {
        let result = SyncResult {
            added: 3,
            removed: 1,
            total: 5,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("added: 3"));
        assert!(debug_str.contains("removed: 1"));
        assert!(debug_str.contains("total: 5"));
    }

    #[test]
    fn test_sync_result_clone() {
        let result = SyncResult {
            added: 2,
            removed: 0,
            total: 4,
        };
        let cloned = result.clone();
        assert_eq!(cloned.added, 2);
        assert_eq!(cloned.removed, 0);
        assert_eq!(cloned.total, 4);
    }

    // ===== update_device_info テスト =====

    #[tokio::test]
    async fn test_update_device_info_existing() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "DeviceInfo".to_string(),
            "http://localhost:9027".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        let device_info = crate::types::endpoint::DeviceInfo {
            device_type: crate::types::endpoint::DeviceType::Gpu,
            gpu_devices: vec![],
        };

        let result = registry
            .update_device_info(ep_id, Some(device_info))
            .await
            .unwrap();
        assert!(result);

        let updated = registry.get(ep_id).await.unwrap();
        assert!(updated.device_info.is_some());
    }

    #[tokio::test]
    async fn test_update_device_info_nonexistent() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let result = registry
            .update_device_info(Uuid::new_v4(), None)
            .await
            .unwrap();
        assert!(!result);
    }

    // ===== increment_request_counters 追加テスト =====

    #[tokio::test]
    async fn test_increment_request_counters_multiple_times() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let ep = Endpoint::new(
            "MultiIncrement".to_string(),
            "http://localhost:9028".to_string(),
            EndpointType::Xllm,
        );
        let ep_id = ep.id;
        registry.add(ep).await.unwrap();

        // 3回成功、2回失敗
        for _ in 0..3 {
            registry
                .increment_request_counters(ep_id, true)
                .await
                .unwrap();
        }
        for _ in 0..2 {
            registry
                .increment_request_counters(ep_id, false)
                .await
                .unwrap();
        }

        let updated = registry.get(ep_id).await.unwrap();
        assert_eq!(updated.total_requests, 5);
        assert_eq!(updated.successful_requests, 3);
        assert_eq!(updated.failed_requests, 2);
    }

    // ===== has_capability_online 追加テスト =====

    #[tokio::test]
    async fn test_has_capability_online_empty_registry() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        assert!(
            !registry
                .has_capability_online(EndpointCapability::ChatCompletion)
                .await
        );
    }
}
