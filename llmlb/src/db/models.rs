//! モデル情報の永続化 (SQLite)

use crate::common::error::{LbError, RouterResult};
use crate::types::ModelCapability;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use std::collections::HashMap;

use crate::types::model::{ModelInfo, ModelSource};

/// SQLiteベースのモデルストレージ
#[derive(Clone)]
pub struct ModelStorage {
    pool: SqlitePool,
}

/// データベース行からの読み取り用構造体
#[derive(Debug, sqlx::FromRow)]
struct ModelRow {
    name: String,
    size: i64,
    description: String,
    required_memory: i64,
    source: String,
    chat_template: Option<String>,
    repo: Option<String>,
    filename: Option<String>,
    last_modified: Option<String>,
    status: Option<String>,
}

impl ModelStorage {
    /// 新しいModelStorageを作成
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// モデルを保存（UPSERT）
    pub async fn save_model(&self, model: &ModelInfo) -> RouterResult<()> {
        let source_str = match model.source {
            ModelSource::Predefined => "predefined",
            ModelSource::HfGguf => "hf_gguf",
            ModelSource::HfSafetensors => "hf_safetensors",
            ModelSource::HfOnnx => "hf_onnx",
        };

        let last_modified_str = model.last_modified.map(|dt| dt.to_rfc3339());

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| LbError::Database(format!("Failed to begin transaction: {}", e)))?;

        // メインモデルをUPSERT
        sqlx::query(
            r#"
            INSERT INTO models (name, size, description, required_memory, source,
                               chat_template, repo, filename,
                               last_modified, status)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(name) DO UPDATE SET
                size = excluded.size,
                description = excluded.description,
                required_memory = excluded.required_memory,
                source = excluded.source,
                chat_template = excluded.chat_template,
                repo = excluded.repo,
                filename = excluded.filename,
                last_modified = excluded.last_modified,
                status = excluded.status
            "#,
        )
        .bind(&model.name)
        .bind(model.size as i64)
        .bind(&model.description)
        .bind(model.required_memory as i64)
        .bind(source_str)
        .bind(&model.chat_template)
        .bind(&model.repo)
        .bind(&model.filename)
        .bind(&last_modified_str)
        .bind(&model.status)
        .execute(&mut *tx)
        .await
        .map_err(|e| LbError::Database(format!("Failed to upsert model: {}", e)))?;

        // タグを更新
        self.clear_and_insert_tags(&mut tx, &model.name, &model.tags)
            .await?;

        // 能力を更新
        self.clear_and_insert_capabilities(&mut tx, &model.name, &model.capabilities)
            .await?;

        tx.commit()
            .await
            .map_err(|e| LbError::Database(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }

    /// タグをクリアして再挿入
    async fn clear_and_insert_tags(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        model_name: &str,
        tags: &[String],
    ) -> RouterResult<()> {
        sqlx::query("DELETE FROM model_tags WHERE model_name = ?")
            .bind(model_name)
            .execute(&mut **tx)
            .await
            .map_err(|e| LbError::Database(format!("Failed to delete tags: {}", e)))?;

        for tag in tags {
            sqlx::query("INSERT INTO model_tags (model_name, tag) VALUES (?, ?)")
                .bind(model_name)
                .bind(tag)
                .execute(&mut **tx)
                .await
                .map_err(|e| LbError::Database(format!("Failed to insert tag: {}", e)))?;
        }

        Ok(())
    }

    /// 能力をクリアして再挿入
    async fn clear_and_insert_capabilities(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        model_name: &str,
        capabilities: &[ModelCapability],
    ) -> RouterResult<()> {
        sqlx::query("DELETE FROM model_capabilities WHERE model_name = ?")
            .bind(model_name)
            .execute(&mut **tx)
            .await
            .map_err(|e| LbError::Database(format!("Failed to delete capabilities: {}", e)))?;

        for cap in capabilities {
            let cap_str = format!("{:?}", cap);
            sqlx::query("INSERT INTO model_capabilities (model_name, capability) VALUES (?, ?)")
                .bind(model_name)
                .bind(&cap_str)
                .execute(&mut **tx)
                .await
                .map_err(|e| LbError::Database(format!("Failed to insert capability: {}", e)))?;
        }

        Ok(())
    }

    /// 全モデルを読み込み
    pub async fn load_models(&self) -> RouterResult<Vec<ModelInfo>> {
        let rows: Vec<ModelRow> = sqlx::query_as("SELECT * FROM models")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to load models: {}", e)))?;

        // N+1 回避: 従来は各モデルごとに tags/capabilities を個別クエリ（1 + 2N クエリ）
        // していたが、全 tags/capabilities を1クエリずつ一括取得して model_name で
        // グルーピングする（合計3クエリ）。row.name は models の主キーで一意なため
        // remove で所有権を移してクローンを避ける。
        let mut tags_by_model = self.load_all_tags().await?;
        let mut capabilities_by_model = self.load_all_capabilities().await?;

        let mut models = Vec::with_capacity(rows.len());

        for row in rows {
            let tags = tags_by_model.remove(&row.name).unwrap_or_default();
            let capabilities = capabilities_by_model.remove(&row.name).unwrap_or_default();

            let source = match row.source.as_str() {
                "hf_gguf" => ModelSource::HfGguf,
                "hf_safetensors" => ModelSource::HfSafetensors,
                "hf_onnx" => ModelSource::HfOnnx,
                "hf_pending_conversion" => ModelSource::HfSafetensors,
                _ => ModelSource::Predefined,
            };

            let last_modified = row.last_modified.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });

            models.push(ModelInfo {
                name: row.name,
                size: row.size as u64,
                description: row.description,
                required_memory: row.required_memory as u64,
                tags,
                capabilities,
                source,
                chat_template: row.chat_template,
                repo: row.repo,
                filename: row.filename,
                last_modified,
                status: row.status,
            });
        }

        Ok(models)
    }

    /// 特定のモデルを読み込み
    pub async fn load_model(&self, name: &str) -> RouterResult<Option<ModelInfo>> {
        let row: Option<ModelRow> = sqlx::query_as("SELECT * FROM models WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to load model: {}", e)))?;

        match row {
            Some(row) => {
                let tags = self.load_tags(&row.name).await?;
                let capabilities = self.load_capabilities(&row.name).await?;

                let source = match row.source.as_str() {
                    "hf_gguf" => ModelSource::HfGguf,
                    "hf_safetensors" => ModelSource::HfSafetensors,
                    "hf_onnx" => ModelSource::HfOnnx,
                    "hf_pending_conversion" => ModelSource::HfSafetensors,
                    _ => ModelSource::Predefined,
                };

                let last_modified = row.last_modified.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                });

                Ok(Some(ModelInfo {
                    name: row.name,
                    size: row.size as u64,
                    description: row.description,
                    required_memory: row.required_memory as u64,
                    tags,
                    capabilities,
                    source,
                    chat_template: row.chat_template,
                    repo: row.repo,
                    filename: row.filename,
                    last_modified,
                    status: row.status,
                }))
            }
            None => Ok(None),
        }
    }

    /// モデルを削除
    pub async fn delete_model(&self, name: &str) -> RouterResult<()> {
        sqlx::query("DELETE FROM models WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to delete model: {}", e)))?;

        Ok(())
    }

    /// タグを読み込み
    async fn load_tags(&self, model_name: &str) -> RouterResult<Vec<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT tag FROM model_tags WHERE model_name = ?")
                .bind(model_name)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| LbError::Database(format!("Failed to load tags: {}", e)))?;

        Ok(rows.into_iter().map(|(tag,)| tag).collect())
    }

    /// 能力を読み込み
    async fn load_capabilities(&self, model_name: &str) -> RouterResult<Vec<ModelCapability>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT capability FROM model_capabilities WHERE model_name = ?")
                .bind(model_name)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| LbError::Database(format!("Failed to load capabilities: {}", e)))?;

        let capabilities: Vec<ModelCapability> = rows
            .into_iter()
            .filter_map(|(cap_str,)| parse_capability(&cap_str))
            .collect();

        Ok(capabilities)
    }

    /// 全モデルのタグを1クエリで一括取得し model_name でグルーピング（N+1回避）
    async fn load_all_tags(&self) -> RouterResult<HashMap<String, Vec<String>>> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT model_name, tag FROM model_tags")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to load tags: {}", e)))?;

        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (model_name, tag) in rows {
            map.entry(model_name).or_default().push(tag);
        }
        Ok(map)
    }

    /// 全モデルの能力を1クエリで一括取得し model_name でグルーピング（N+1回避）
    async fn load_all_capabilities(&self) -> RouterResult<HashMap<String, Vec<ModelCapability>>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT model_name, capability FROM model_capabilities")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| LbError::Database(format!("Failed to load capabilities: {}", e)))?;

        let mut map: HashMap<String, Vec<ModelCapability>> = HashMap::new();
        for (model_name, cap_str) in rows {
            if let Some(cap) = parse_capability(&cap_str) {
                map.entry(model_name).or_default().push(cap);
            }
        }
        Ok(map)
    }

    /// 複数モデルを一括保存
    pub async fn save_models(&self, models: &[ModelInfo]) -> RouterResult<()> {
        for model in models {
            self.save_model(model).await?;
        }
        Ok(())
    }
}

/// DB に保存された capability 文字列を `ModelCapability` に変換する。
/// 未知の文字列は `None`（load_capabilities / load_all_capabilities で共有）。
fn parse_capability(cap_str: &str) -> Option<ModelCapability> {
    match cap_str {
        "TextGeneration" => Some(ModelCapability::TextGeneration),
        "TextToSpeech" => Some(ModelCapability::TextToSpeech),
        "SpeechToText" => Some(ModelCapability::SpeechToText),
        "ImageGeneration" => Some(ModelCapability::ImageGeneration),
        "ImageInput" => Some(ModelCapability::ImageInput),
        "Embedding" => Some(ModelCapability::Embedding),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_pool() -> SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    #[tokio::test]
    async fn test_save_and_load_model() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let model = ModelInfo {
            name: "test-model".to_string(),
            size: 1000000,
            description: "Test model".to_string(),
            required_memory: 2000000,
            tags: vec!["llm".to_string(), "test".to_string()],
            capabilities: vec![ModelCapability::TextGeneration],
            source: ModelSource::Predefined,
            chat_template: None,
            repo: Some("test/repo".to_string()),
            filename: Some("model.gguf".to_string()),
            last_modified: Some(Utc::now()),
            status: Some("available".to_string()),
        };

        storage.save_model(&model).await.unwrap();

        let loaded = storage.load_model("test-model").await.unwrap();
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.name, "test-model");
        assert_eq!(loaded.size, 1000000);
        assert_eq!(loaded.tags, vec!["llm", "test"]);
        assert_eq!(loaded.capabilities, vec![ModelCapability::TextGeneration]);
    }

    #[tokio::test]
    async fn test_load_models() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let model1 = ModelInfo::new(
            "model1".to_string(),
            1000,
            "Model 1".to_string(),
            2000,
            vec!["tag1".to_string()],
        );
        let model2 = ModelInfo::new(
            "model2".to_string(),
            2000,
            "Model 2".to_string(),
            4000,
            vec!["tag2".to_string()],
        );

        storage.save_model(&model1).await.unwrap();
        storage.save_model(&model2).await.unwrap();

        let models = storage.load_models().await.unwrap();
        assert_eq!(models.len(), 2);
    }

    #[tokio::test]
    async fn test_load_models_groups_tags_and_capabilities_per_model() {
        // N+1 一括取得版で tags/capabilities がモデル間で混線しないことを固定する
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let mut model1 = ModelInfo::new(
            "m1".to_string(),
            1000,
            "M1".to_string(),
            2000,
            vec!["a".to_string(), "b".to_string()],
        );
        model1.capabilities = vec![ModelCapability::TextGeneration, ModelCapability::Embedding];

        let mut model2 = ModelInfo::new(
            "m2".to_string(),
            2000,
            "M2".to_string(),
            4000,
            vec!["c".to_string()],
        );
        model2.capabilities = vec![ModelCapability::ImageInput];

        // タグも能力も持たないモデル
        let mut model3 = ModelInfo::new("m3".to_string(), 3000, "M3".to_string(), 6000, vec![]);
        model3.capabilities = vec![];

        storage.save_model(&model1).await.unwrap();
        storage.save_model(&model2).await.unwrap();
        storage.save_model(&model3).await.unwrap();

        let models = storage.load_models().await.unwrap();
        assert_eq!(models.len(), 3);

        let sorted = |mut v: Vec<String>| {
            v.sort();
            v
        };
        let get = |name: &str| models.iter().find(|m| m.name == name).unwrap().clone();

        let cap_set = |caps: &[ModelCapability]| {
            caps.iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
        };

        let m1 = get("m1");
        assert_eq!(
            sorted(m1.tags.clone()),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            cap_set(&m1.capabilities),
            cap_set(&[ModelCapability::TextGeneration, ModelCapability::Embedding])
        );

        let m2 = get("m2");
        assert_eq!(m2.tags, vec!["c".to_string()]);
        assert_eq!(m2.capabilities, vec![ModelCapability::ImageInput]);

        let m3 = get("m3");
        assert!(m3.tags.is_empty());
        assert!(m3.capabilities.is_empty());
    }

    #[tokio::test]
    async fn test_delete_model() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let model = ModelInfo::new(
            "to-delete".to_string(),
            1000,
            "To delete".to_string(),
            2000,
            vec![],
        );

        storage.save_model(&model).await.unwrap();
        assert!(storage.load_model("to-delete").await.unwrap().is_some());

        storage.delete_model("to-delete").await.unwrap();
        assert!(storage.load_model("to-delete").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_update_model() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let mut model = ModelInfo::new(
            "updatable".to_string(),
            1000,
            "Original".to_string(),
            2000,
            vec!["original".to_string()],
        );

        storage.save_model(&model).await.unwrap();

        // Update the model
        model.description = "Updated".to_string();
        model.tags = vec!["updated".to_string()];
        model.capabilities = vec![ModelCapability::TextGeneration];

        storage.save_model(&model).await.unwrap();

        let loaded = storage.load_model("updatable").await.unwrap().unwrap();
        assert_eq!(loaded.description, "Updated");
        assert_eq!(loaded.tags, vec!["updated"]);
        assert_eq!(loaded.capabilities, vec![ModelCapability::TextGeneration]);
    }

    #[tokio::test]
    async fn test_load_model_nonexistent() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);
        let result = storage.load_model("no-such-model").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_model_succeeds() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);
        // Deleting a non-existent model should not error
        storage.delete_model("phantom").await.unwrap();
    }

    #[tokio::test]
    async fn test_save_model_all_sources() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let sources = [
            ("m-predefined", ModelSource::Predefined),
            ("m-hf-gguf", ModelSource::HfGguf),
            ("m-hf-safetensors", ModelSource::HfSafetensors),
            ("m-hf-onnx", ModelSource::HfOnnx),
        ];

        for (name, source) in &sources {
            let mut model = ModelInfo::new(name.to_string(), 100, "desc".to_string(), 200, vec![]);
            model.source = source.clone();
            storage.save_model(&model).await.unwrap();
        }

        let models = storage.load_models().await.unwrap();
        assert_eq!(models.len(), 4);
    }

    #[tokio::test]
    async fn test_save_model_multiple_capabilities() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let model = ModelInfo {
            name: "multi-cap".to_string(),
            size: 500,
            description: "Multi capability".to_string(),
            required_memory: 1000,
            tags: vec![],
            capabilities: vec![ModelCapability::TextGeneration, ModelCapability::Embedding],
            source: ModelSource::Predefined,
            chat_template: None,
            repo: None,
            filename: None,
            last_modified: None,
            status: None,
        };

        storage.save_model(&model).await.unwrap();
        let loaded = storage.load_model("multi-cap").await.unwrap().unwrap();
        assert_eq!(loaded.capabilities.len(), 2);
        assert!(loaded
            .capabilities
            .contains(&ModelCapability::TextGeneration));
        assert!(loaded.capabilities.contains(&ModelCapability::Embedding));
    }

    #[tokio::test]
    async fn test_save_models_batch() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let models: Vec<ModelInfo> = (0..5)
            .map(|i| {
                ModelInfo::new(
                    format!("batch-{}", i),
                    100 * (i + 1),
                    format!("Model {}", i),
                    200 * (i + 1),
                    vec![format!("tag-{}", i)],
                )
            })
            .collect();

        storage.save_models(&models).await.unwrap();

        let loaded = storage.load_models().await.unwrap();
        assert_eq!(loaded.len(), 5);
    }

    #[tokio::test]
    async fn test_save_model_with_optional_fields() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let model = ModelInfo {
            name: "full-model".to_string(),
            size: 999,
            description: "Full".to_string(),
            required_memory: 1998,
            tags: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            capabilities: vec![ModelCapability::TextGeneration],
            source: ModelSource::HfGguf,
            chat_template: Some("chatml".to_string()),
            repo: Some("org/repo".to_string()),
            filename: Some("model-q4.gguf".to_string()),
            last_modified: Some(Utc::now()),
            status: Some("ready".to_string()),
        };

        storage.save_model(&model).await.unwrap();
        let loaded = storage.load_model("full-model").await.unwrap().unwrap();
        assert_eq!(loaded.chat_template, Some("chatml".to_string()));
        assert_eq!(loaded.repo, Some("org/repo".to_string()));
        assert_eq!(loaded.filename, Some("model-q4.gguf".to_string()));
        assert!(loaded.last_modified.is_some());
        assert_eq!(loaded.status, Some("ready".to_string()));
        assert_eq!(loaded.tags.len(), 3);
    }

    #[tokio::test]
    async fn test_upsert_preserves_name() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let model = ModelInfo::new(
            "upsert-test".to_string(),
            100,
            "v1".to_string(),
            200,
            vec![],
        );
        storage.save_model(&model).await.unwrap();

        let mut model2 = ModelInfo::new(
            "upsert-test".to_string(),
            999,
            "v2".to_string(),
            1998,
            vec!["new-tag".to_string()],
        );
        model2.source = ModelSource::HfSafetensors;
        storage.save_model(&model2).await.unwrap();

        let models = storage.load_models().await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].description, "v2");
        assert_eq!(models[0].size, 999);
    }

    #[tokio::test]
    async fn test_empty_load_models() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);
        let models = storage.load_models().await.unwrap();
        assert!(models.is_empty());
    }

    #[tokio::test]
    async fn test_save_model_no_tags_no_capabilities() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let model = ModelInfo {
            name: "bare-model".to_string(),
            size: 100,
            description: "No tags or caps".to_string(),
            required_memory: 200,
            tags: vec![],
            capabilities: vec![],
            source: ModelSource::Predefined,
            chat_template: None,
            repo: None,
            filename: None,
            last_modified: None,
            status: None,
        };

        storage.save_model(&model).await.unwrap();
        let loaded = storage.load_model("bare-model").await.unwrap().unwrap();
        assert!(loaded.tags.is_empty());
        assert!(loaded.capabilities.is_empty());
    }

    #[tokio::test]
    async fn test_upsert_updates_tags_completely() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let mut model = ModelInfo::new(
            "tag-update".to_string(),
            100,
            "desc".to_string(),
            200,
            vec!["old-tag-1".to_string(), "old-tag-2".to_string()],
        );
        storage.save_model(&model).await.unwrap();

        // Update with completely different tags
        model.tags = vec!["new-tag".to_string()];
        storage.save_model(&model).await.unwrap();

        let loaded = storage.load_model("tag-update").await.unwrap().unwrap();
        assert_eq!(loaded.tags, vec!["new-tag"]);
    }

    #[tokio::test]
    async fn test_upsert_updates_capabilities_completely() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let mut model = ModelInfo::new(
            "cap-update".to_string(),
            100,
            "desc".to_string(),
            200,
            vec![],
        );
        model.capabilities = vec![ModelCapability::TextGeneration, ModelCapability::Embedding];
        storage.save_model(&model).await.unwrap();

        // Replace capabilities
        model.capabilities = vec![ModelCapability::SpeechToText];
        storage.save_model(&model).await.unwrap();

        let loaded = storage.load_model("cap-update").await.unwrap().unwrap();
        assert_eq!(loaded.capabilities, vec![ModelCapability::SpeechToText]);
    }

    #[tokio::test]
    async fn test_delete_model_removes_tags_and_capabilities() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let model = ModelInfo {
            name: "delete-cascade".to_string(),
            size: 100,
            description: "desc".to_string(),
            required_memory: 200,
            tags: vec!["tag1".to_string()],
            capabilities: vec![ModelCapability::TextGeneration],
            source: ModelSource::Predefined,
            chat_template: None,
            repo: None,
            filename: None,
            last_modified: None,
            status: None,
        };
        storage.save_model(&model).await.unwrap();
        storage.delete_model("delete-cascade").await.unwrap();

        // Re-create with same name should work without conflicts
        let model2 = ModelInfo::new(
            "delete-cascade".to_string(),
            500,
            "new desc".to_string(),
            1000,
            vec!["new-tag".to_string()],
        );
        storage.save_model(&model2).await.unwrap();
        let loaded = storage.load_model("delete-cascade").await.unwrap().unwrap();
        assert_eq!(loaded.size, 500);
        assert_eq!(loaded.tags, vec!["new-tag"]);
    }

    #[tokio::test]
    async fn test_all_model_capabilities() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let all_caps = vec![
            ModelCapability::TextGeneration,
            ModelCapability::TextToSpeech,
            ModelCapability::SpeechToText,
            ModelCapability::ImageGeneration,
            ModelCapability::ImageInput,
            ModelCapability::Embedding,
        ];

        let model = ModelInfo {
            name: "all-caps".to_string(),
            size: 100,
            description: "all capabilities".to_string(),
            required_memory: 200,
            tags: vec![],
            capabilities: all_caps.clone(),
            source: ModelSource::Predefined,
            chat_template: None,
            repo: None,
            filename: None,
            last_modified: None,
            status: None,
        };

        storage.save_model(&model).await.unwrap();
        let loaded = storage.load_model("all-caps").await.unwrap().unwrap();
        assert_eq!(loaded.capabilities.len(), 6);
        for cap in &all_caps {
            assert!(loaded.capabilities.contains(cap));
        }
    }

    #[tokio::test]
    async fn test_save_model_hf_safetensors_source() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let mut model = ModelInfo::new(
            "st-model".to_string(),
            1000,
            "SafeTensors model".to_string(),
            2000,
            vec![],
        );
        model.source = ModelSource::HfSafetensors;
        storage.save_model(&model).await.unwrap();

        let loaded = storage.load_model("st-model").await.unwrap().unwrap();
        assert_eq!(loaded.source, ModelSource::HfSafetensors);
    }

    #[tokio::test]
    async fn test_save_model_hf_onnx_source() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let mut model = ModelInfo::new(
            "onnx-model".to_string(),
            500,
            "ONNX model".to_string(),
            1000,
            vec![],
        );
        model.source = ModelSource::HfOnnx;
        storage.save_model(&model).await.unwrap();

        let loaded = storage.load_model("onnx-model").await.unwrap().unwrap();
        assert_eq!(loaded.source, ModelSource::HfOnnx);
    }

    #[tokio::test]
    async fn test_model_last_modified_roundtrip() {
        let pool = create_test_pool().await;
        let storage = ModelStorage::new(pool);

        let now = Utc::now();
        let model = ModelInfo {
            name: "time-model".to_string(),
            size: 100,
            description: "desc".to_string(),
            required_memory: 200,
            tags: vec![],
            capabilities: vec![],
            source: ModelSource::Predefined,
            chat_template: None,
            repo: None,
            filename: None,
            last_modified: Some(now),
            status: None,
        };

        storage.save_model(&model).await.unwrap();
        let loaded = storage.load_model("time-model").await.unwrap().unwrap();
        assert!(loaded.last_modified.is_some());
        // Within 1 second tolerance due to RFC3339 precision
        let diff = (loaded.last_modified.unwrap() - now).num_seconds().abs();
        assert!(diff <= 1);
    }
}
