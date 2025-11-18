// 認証モジュール

/// パスワードハッシュ化・検証（bcrypt）
pub mod password;

/// JWT生成・検証（jsonwebtoken）
pub mod jwt;

/// 認証ミドルウェア（JWT, APIキー, エージェントトークン）
pub mod middleware;

/// 初回起動時の管理者アカウント作成
pub mod bootstrap;

/// ランダムトークン生成
pub fn generate_random_token(length: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}
