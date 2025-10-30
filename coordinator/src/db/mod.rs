//! データベースアクセス層
//!
//! SQLiteデータベースへの接続とクエリ実行

use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use ollama_coordinator_common::{
    types::{Agent, AgentStatus},
    error::{CoordinatorError, CoordinatorResult},
};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// データベース接続プールを作成
pub async fn create_pool(database_url: &str) -> CoordinatorResult<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|e| CoordinatorError::Database(e.to_string()))?;

    // マイグレーション実行
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Migration failed: {}", e)))?;

    Ok(pool)
}

/// エージェントをデータベースに保存
pub async fn save_agent(pool: &SqlitePool, agent: &Agent) -> CoordinatorResult<()> {
    let status_str = match agent.status {
        AgentStatus::Online => "online",
        AgentStatus::Offline => "offline",
    };

    sqlx::query(
        r#"
        INSERT INTO agents (id, machine_name, ip_address, ollama_version, ollama_port, status, registered_at, last_seen)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            machine_name = excluded.machine_name,
            ip_address = excluded.ip_address,
            ollama_version = excluded.ollama_version,
            ollama_port = excluded.ollama_port,
            status = excluded.status,
            last_seen = excluded.last_seen
        "#
    )
    .bind(agent.id.to_string())
    .bind(&agent.machine_name)
    .bind(agent.ip_address.to_string())
    .bind(&agent.ollama_version)
    .bind(agent.ollama_port as i64)
    .bind(status_str)
    .bind(agent.registered_at.to_rfc3339())
    .bind(agent.last_seen.to_rfc3339())
    .execute(pool)
    .await
    .map_err(|e| CoordinatorError::Database(format!("Failed to save agent: {}", e)))?;

    Ok(())
}

/// データベースから全エージェントを読み込み
pub async fn load_agents(pool: &SqlitePool) -> CoordinatorResult<Vec<Agent>> {
    let rows = sqlx::query("SELECT id, machine_name, ip_address, ollama_version, ollama_port, status, registered_at, last_seen FROM agents")
        .fetch_all(pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to load agents: {}", e)))?;

    let mut agents = Vec::new();

    for row in rows {
        let id_str: String = row.get("id");
        let id = Uuid::parse_str(&id_str)
            .map_err(|e| CoordinatorError::Database(format!("Invalid UUID: {}", e)))?;

        let machine_name: String = row.get("machine_name");

        let ip_str: String = row.get("ip_address");
        let ip_address = ip_str.parse()
            .map_err(|e| CoordinatorError::Database(format!("Invalid IP address: {}", e)))?;

        let ollama_version: String = row.get("ollama_version");
        let ollama_port: i64 = row.get("ollama_port");

        let status_str: String = row.get("status");
        let status = match status_str.as_str() {
            "online" => AgentStatus::Online,
            "offline" => AgentStatus::Offline,
            _ => AgentStatus::Offline,
        };

        let registered_at_str: String = row.get("registered_at");
        let registered_at = DateTime::parse_from_rfc3339(&registered_at_str)
            .map_err(|e| CoordinatorError::Database(format!("Invalid datetime: {}", e)))?
            .with_timezone(&Utc);

        let last_seen_str: String = row.get("last_seen");
        let last_seen = DateTime::parse_from_rfc3339(&last_seen_str)
            .map_err(|e| CoordinatorError::Database(format!("Invalid datetime: {}", e)))?
            .with_timezone(&Utc);

        agents.push(Agent {
            id,
            machine_name,
            ip_address,
            ollama_version,
            ollama_port: ollama_port as u16,
            status,
            registered_at,
            last_seen,
        });
    }

    Ok(agents)
}

/// エージェントをデータベースから削除
pub async fn delete_agent(pool: &SqlitePool, agent_id: Uuid) -> CoordinatorResult<()> {
    sqlx::query("DELETE FROM agents WHERE id = ?")
        .bind(agent_id.to_string())
        .execute(pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to delete agent: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[tokio::test]
    async fn test_create_pool_with_invalid_url() {
        let result = create_pool("invalid://url").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoordinatorError::Database(_)));
    }

    #[tokio::test]
    async fn test_save_and_load_agent() {
        let pool = create_pool("sqlite::memory:").await.unwrap();

        let agent = Agent {
            id: Uuid::new_v4(),
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            status: AgentStatus::Online,
            registered_at: Utc::now(),
            last_seen: Utc::now(),
        };

        // 保存
        save_agent(&pool, &agent).await.unwrap();

        // 読み込み
        let loaded_agents = load_agents(&pool).await.unwrap();
        assert_eq!(loaded_agents.len(), 1);
        assert_eq!(loaded_agents[0].id, agent.id);
        assert_eq!(loaded_agents[0].machine_name, agent.machine_name);
    }

    #[tokio::test]
    async fn test_delete_agent() {
        let pool = create_pool("sqlite::memory:").await.unwrap();

        let agent = Agent {
            id: Uuid::new_v4(),
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            status: AgentStatus::Online,
            registered_at: Utc::now(),
            last_seen: Utc::now(),
        };

        save_agent(&pool, &agent).await.unwrap();
        delete_agent(&pool, agent.id).await.unwrap();

        let loaded_agents = load_agents(&pool).await.unwrap();
        assert_eq!(loaded_agents.len(), 0);
    }
}
