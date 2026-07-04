//! 型定義モジュール
//!
//! ドメインエンティティの型定義を提供

/// エンドポイント関連の型定義
pub mod endpoint;

/// ヘルスメトリクス型定義
pub mod health;

/// メディア関連型定義（音声・画像）
pub mod media;

/// モデル関連型定義
pub mod model;

pub use endpoint::{Endpoint, EndpointHealthCheck, EndpointModel, EndpointStatus, SupportedAPI};
pub use health::{HealthMetrics, Request, RequestStatus};
pub use media::{AudioFormat, ImageQuality, ImageResponseFormat, ImageSize, ImageStyle};
pub use model::{
    ModelCapabilities, ModelCapability, ModelInfo, ModelSource, ModelType, SyncProgress, SyncState,
};
