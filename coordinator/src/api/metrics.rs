//! エージェントメトリクスAPIハンドラー

use crate::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use ollama_coordinator_common::types::AgentMetrics;

use super::agent::AppError;

/// POST /api/agents/:id/metrics - エージェントメトリクス更新
///
/// エージェントから送信されたメトリクス情報（CPU使用率、メモリ使用率、アクティブリクエスト数等）を
/// メモリ内のHashMapに保存する。エージェントが存在しない場合は404を返す。
pub async fn update_metrics(
    State(state): State<AppState>,
    axum::extract::Path(agent_id): axum::extract::Path<uuid::Uuid>,
    Json(mut metrics): Json<AgentMetrics>,
) -> Result<impl IntoResponse, AppError> {
    // パスパラメータのagent_idとリクエストボディのagent_idを統一
    metrics.agent_id = agent_id;

    // registryのupdate_metrics()を呼び出し
    state.registry.update_metrics(metrics).await?;

    // 204 No Content を返す
    Ok(StatusCode::NO_CONTENT)
}
