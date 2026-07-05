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
mod tests;
