//! デバッグビルド用の固定 API キーと権限解決
//!
//! arch-review [H6] round2: auth/middleware.rs からデバッグキー定義を分離。
//! #[cfg(debug_assertions)] で保護され、リリースビルドでは無効。

#[cfg(debug_assertions)]
pub(crate) const DEBUG_API_KEY_ALL: &str = "sk_debug";
#[cfg(debug_assertions)]
pub(crate) const DEBUG_API_KEY_RUNTIME: &str = "sk_debug_runtime";
#[cfg(debug_assertions)]
pub(crate) const DEBUG_API_KEY_API: &str = "sk_debug_api";
#[cfg(debug_assertions)]
pub(crate) const DEBUG_API_KEY_ADMIN: &str = "sk_debug_admin";

#[cfg(debug_assertions)]
pub(crate) fn debug_api_key_permissions(
    request_key: &str,
) -> Option<Vec<crate::common::auth::ApiKeyPermission>> {
    match request_key {
        DEBUG_API_KEY_ALL => Some(crate::common::auth::ApiKeyPermission::all()),
        DEBUG_API_KEY_RUNTIME => Some(vec![crate::common::auth::ApiKeyPermission::RegistryRead]),
        DEBUG_API_KEY_API => Some(vec![
            crate::common::auth::ApiKeyPermission::OpenaiInference,
            crate::common::auth::ApiKeyPermission::OpenaiModelsRead,
        ]),
        DEBUG_API_KEY_ADMIN => Some(crate::common::auth::ApiKeyPermission::all()),
        _ => None,
    }
}

#[cfg(not(debug_assertions))]
pub(crate) fn debug_api_key_permissions(
    _request_key: &str,
) -> Option<Vec<crate::common::auth::ApiKeyPermission>> {
    None
}
