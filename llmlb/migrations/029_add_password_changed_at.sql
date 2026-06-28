-- パスワード最終変更時刻（epochミリ秒）。JWTセッション無効化判定に使用する。
-- 既存ユーザーはバックフィルせず 0 のままにする（既発行トークンの password_changed_at も
-- serde default で 0 になるため、デプロイ直後の予期せぬ全ログアウトを避ける）。
-- 初回のパスワード変更/リセットで現在時刻に更新され、以降は古いセッションを無効化できる。
ALTER TABLE users ADD COLUMN password_changed_at INTEGER NOT NULL DEFAULT 0;
