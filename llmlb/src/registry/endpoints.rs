//! エンドポイントレジストリ
//!
//! エンドポイントの状態をメモリ内で管理し、SQLiteと同期

use crate::db::endpoints as db;
use crate::types::endpoint::Endpoint;
#[cfg(test)]
use crate::types::endpoint::{EndpointModel, EndpointStatus, EndpointType};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

mod model_map;
mod model_sync;
mod mutations;
mod queries;
use model_map::insert_model_mapping;

use uuid::Uuid;

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
mod tests;
