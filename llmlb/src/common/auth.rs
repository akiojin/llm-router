// T032-T037: 認証関連のデータモデル（最小実装、テスト用）

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// ユーザーロール
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    /// 管理者（全操作可能）
    Admin,
    /// 閲覧者（読み取りのみ）
    Viewer,
}

/// ユーザー
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// ユーザーID
    pub id: Uuid,
    /// ユーザー名
    pub username: String,
    /// パスワードハッシュ（bcrypt）
    pub password_hash: String,
    /// ユーザーロール
    pub role: UserRole,
    /// 作成日時
    pub created_at: DateTime<Utc>,
    /// 最終ログイン日時
    pub last_login: Option<DateTime<Utc>>,
    /// 初回パスワード変更が必要かどうか
    pub must_change_password: bool,
    /// パスワード最終変更時刻（epochミリ秒）。JWTセッション無効化判定に使用する
    #[serde(default)]
    pub password_changed_at: i64,
}

/// APIキー
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// APIキーID
    pub id: Uuid,
    /// キーのSHA-256ハッシュ
    pub key_hash: String,
    /// キーの先頭部分（表示用）
    pub key_prefix: Option<String>,
    /// キーの名前
    pub name: String,
    /// 作成者のユーザーID
    pub created_by: Uuid,
    /// 作成日時
    pub created_at: DateTime<Utc>,
    /// 有効期限
    pub expires_at: Option<DateTime<Utc>>,
    /// 付与された権限
    pub permissions: Vec<ApiKeyPermission>,
}

/// APIキー権限
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApiKeyPermission {
    /// OpenAI互換APIの推論（/v1/chat/completions 等）
    #[serde(rename = "openai.inference")]
    OpenaiInference,
    /// OpenAI互換のモデル一覧参照（/v1/models）
    #[serde(rename = "openai.models.read")]
    OpenaiModelsRead,
    /// エンドポイント参照（/api/endpoints のGET系）
    #[serde(rename = "endpoints.read")]
    EndpointsRead,
    /// エンドポイント管理（/api/endpoints の作成/更新/削除 等）
    #[serde(rename = "endpoints.manage")]
    EndpointsManage,
    /// APIキー管理（発行/更新/削除）
    #[serde(rename = "api_keys.manage")]
    ApiKeysManage,
    /// ユーザー管理
    #[serde(rename = "users.manage")]
    UsersManage,
    /// 招待管理
    #[serde(rename = "invitations.manage")]
    InvitationsManage,
    /// モデル管理（register/delete 等）
    #[serde(rename = "models.manage")]
    ModelsManage,
    /// モデル配布レジストリ参照（/api/models/registry/*）
    #[serde(rename = "registry.read")]
    RegistryRead,
    /// ログ参照（lb/node logs）
    #[serde(rename = "logs.read")]
    LogsRead,
    /// メトリクス参照（/api/metrics/*）
    #[serde(rename = "metrics.read")]
    MetricsRead,
}

impl ApiKeyPermission {
    /// すべての権限
    pub fn all() -> Vec<ApiKeyPermission> {
        vec![
            ApiKeyPermission::OpenaiInference,
            ApiKeyPermission::OpenaiModelsRead,
            ApiKeyPermission::EndpointsRead,
            ApiKeyPermission::EndpointsManage,
            ApiKeyPermission::ApiKeysManage,
            ApiKeyPermission::UsersManage,
            ApiKeyPermission::InvitationsManage,
            ApiKeyPermission::ModelsManage,
            ApiKeyPermission::RegistryRead,
            ApiKeyPermission::LogsRead,
            ApiKeyPermission::MetricsRead,
        ]
    }
}

/// APIキー（平文付き、発行時のレスポンス用）
#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyWithPlaintext {
    /// APIキーID
    pub id: Uuid,
    /// 平文のAPIキー（発行時のみ表示）
    pub key: String,
    /// キーの先頭部分（表示用）
    pub key_prefix: String,
    /// キーの名前
    pub name: String,
    /// 作成日時
    pub created_at: DateTime<Utc>,
    /// 有効期限
    pub expires_at: Option<DateTime<Utc>>,
    /// 付与された権限
    pub permissions: Vec<ApiKeyPermission>,
}

/// ランタイムトークン
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeToken {
    /// ランタイムID
    pub runtime_id: Uuid,
    /// トークンのSHA-256ハッシュ
    pub token_hash: String,
    /// 作成日時
    pub created_at: DateTime<Utc>,
}

/// ランタイムトークン（平文付き、発行時のレスポンス用）
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeTokenWithPlaintext {
    /// ランタイムID
    pub runtime_id: Uuid,
    /// 平文のトークン（発行時のみ表示）
    pub token: String,
    /// 作成日時
    pub created_at: DateTime<Utc>,
}

/// JWTクレーム
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claims {
    /// ユーザーID（JWT sub claim）
    pub sub: String,
    /// ユーザーロール
    pub role: UserRole,
    /// 有効期限（Unix timestamp、JWT exp claim）
    pub exp: usize,
    /// パスワード変更が必要か
    #[serde(default)]
    pub must_change_password: bool,
    /// 発行時点のパスワード最終変更時刻（epochミリ秒）。
    /// DB の現在値より小さい場合、そのセッションは無効化されている（パスワード変更/リセット後）。
    #[serde(default)]
    pub password_changed_at: i64,
    /// 表示用ユーザー名（監査ログの actor_username に使用）。
    ///
    /// arch-review [L8]: `sub` はユーザーIDのため、監査ログの actor_username が常に None
    /// だった。認証時にユーザー名を JWT へ載せて actor context として伝搬する。
    /// 旧トークン互換のため `serde(default)`。
    #[serde(default)]
    pub username: Option<String>,
}

#[cfg(test)]
mod tests;
