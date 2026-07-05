//! エンドポイントのモデル一覧同期とマッピング更新。
//!
//! エンドポイントが申告するモデル集合を DB・キャッシュ・逆引きマップへ反映する。

use super::model_map::{insert_model_mapping, remove_model_mapping};
use super::{EndpointRegistry, SyncResult};
use crate::db::endpoints as db;
use crate::types::endpoint::EndpointModel;
use tracing::{debug, warn};
use uuid::Uuid;

/// 単一エンドポイントが「異常な数」のモデルを申告したと判定する閾値。
/// この値を超えたら `warn!()` ログで運用者に通知する（実態を持たない誤申告の早期検知用）。
const SUSPICIOUS_MODEL_COUNT_THRESHOLD: usize = 50;

impl EndpointRegistry {
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
}
