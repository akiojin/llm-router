# クイックスタート: 認証機能

**機能ID**: SPEC-d4eb8796
**対象**: 管理者、開発者

## 前提条件

- llm-router v1.6.0+ がインストール済み
- ポート 8080 が利用可能

## 1. 初回起動と管理者作成

### 方法A: 環境変数で管理者作成（推奨）

```bash
export ADMIN_USERNAME=admin
export ADMIN_PASSWORD=secure123
cargo run --bin coordinator
```

または、起動スクリプトに記述：

```bash
# start-coordinator.sh
#!/bin/bash
export ADMIN_USERNAME=admin
export ADMIN_PASSWORD=your_secure_password_here
./coordinator
```

### 方法B: 対話式で管理者作成

```bash
cargo run --bin coordinator
```

初回起動時にプロンプトが表示されます：

```
[INFO] 認証機能が有効です。管理者ユーザーを作成してください。
ユーザー名: admin
パスワード: ********
[INFO] 管理者ユーザー 'admin' を作成しました。
[INFO] コーディネーターを起動しています...
[INFO] サーバーがポート 8080 で起動しました
```

## 2. ダッシュボードログイン

1. ブラウザで [http://localhost:8080/dashboard](http://localhost:8080/dashboard) にアクセス
2. ログイン画面が表示される
3. 作成したユーザー名とパスワードを入力
4. 「ログイン」ボタンをクリック
5. ダッシュボードが表示される

**テスト検証**: ログイン後、右上にユーザー名が表示されることを確認

## 3. APIキー発行

### ダッシュボードから発行

1. ダッシュボードの「APIキー」タブをクリック
2. 「新規発行」ボタンをクリック
3. キー名を入力（例: `my-chatbot`）
4. 有効期限を設定（オプション、デフォルトは無期限）
5. 「発行」ボタンをクリック
6. 発行されたAPIキー（`sk_xxxxx...`）が表示される
7. **重要**: このキーをコピーして安全な場所に保存（二度と表示されない）

**テスト検証**: 発行後、「APIキー一覧」にキー名と発行日時が表示されることを確認

### APIから発行（プログラム的）

```bash
# ログインしてJWTトークンを取得
JWT_TOKEN=$(curl -X POST http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"secure123"}' \
  | jq -r '.token')

# APIキー発行
API_KEY=$(curl -X POST http://localhost:8080/api/api-keys \
  -H "Authorization: Bearer $JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"my-chatbot"}' \
  | jq -r '.key')

echo "発行されたAPIキー: $API_KEY"
```

## 4. 外部アプリケーションからのアクセス

### OpenAI互換APIを使用

```bash
# チャット補完API
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer sk_xxxxx..." \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-oss:7b",
    "messages": [
      {"role": "user", "content": "Hello, how are you?"}
    ]
  }'
```

**テスト検証**: レスポンスが正常に返ることを確認

### 無効なAPIキーでのアクセス（失敗テスト）

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer invalid_key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-oss:7b",
    "messages": [{"role": "user", "content": "Test"}]
  }'
```

**期待される結果**: `401 Unauthorized` エラーが返る

## 5. ユーザー管理

### 新規ユーザー作成

```bash
# ログインしてJWTトークンを取得
JWT_TOKEN=$(curl -X POST http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"secure123"}' \
  | jq -r '.token')

# 新規ユーザー作成
curl -X POST http://localhost:8080/api/users \
  -H "Authorization: Bearer $JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "username": "viewer_user",
    "password": "password123",
    "role": "viewer"
  }'
```

### ユーザー一覧表示

```bash
curl -X GET http://localhost:8080/api/users \
  -H "Authorization: Bearer $JWT_TOKEN"
```

**テスト検証**: 管理者ユーザーと新規作成したユーザーの両方が表示されることを確認

### パスワード変更

```bash
curl -X PUT http://localhost:8080/api/users/{user_id} \
  -H "Authorization: Bearer $JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"password": "new_password456"}'
```

## 6. エージェント登録とトークン使用

### エージェント登録

```bash
# エージェント登録APIを呼び出す
RESPONSE=$(curl -X POST http://localhost:8080/api/agents \
  -H "Content-Type: application/json" \
  -d '{
    "machine_name": "test-agent",
    "ip_address": "192.168.1.100",
    "ollama_version": "0.1.0",
    "ollama_port": 11434,
    "gpu_available": true,
    "gpu_devices": [{
      "name": "NVIDIA RTX 3090",
      "memory_total": 24000000000,
      "memory_free": 20000000000
    }]
  }')

# レスポンスからエージェントトークンを抽出
AGENT_TOKEN=$(echo $RESPONSE | jq -r '.agent_token')
echo "エージェントトークン: $AGENT_TOKEN"
```

**テスト検証**: レスポンスに `agent_token` フィールドが含まれることを確認

### エージェントからのヘルスチェック

```bash
# エージェントトークンを使用してヘルスチェック
curl -X POST http://localhost:8080/api/health \
  -H "X-Agent-Token: $AGENT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "agent_id": "{エージェントID}",
    "status": "online"
  }'
```

**テスト検証**: `200 OK` が返ることを確認

### トークンなしでのアクセス（失敗テスト）

```bash
curl -X POST http://localhost:8080/api/health \
  -H "Content-Type: application/json" \
  -d '{
    "agent_id": "{エージェントID}",
    "status": "online"
  }'
```

**期待される結果**: `401 Unauthorized` エラーが返る

## 7. 認証無効化モード（プライベートネットワーク用）

### 環境変数で無効化

```bash
export AUTH_DISABLED=true
cargo run --bin coordinator
```

### 動作確認

```bash
# 認証なしでAPIにアクセス
curl -X GET http://localhost:8080/api/agents

# ダッシュボードにログインなしでアクセス可能
open http://localhost:8080/dashboard
```

**テスト検証**: 認証なしですべてのAPIとダッシュボードにアクセスできることを確認

### 認証有効化に戻す

```bash
unset AUTH_DISABLED
# または
export AUTH_DISABLED=false

# コーディネーター再起動
cargo run --bin coordinator
```

## 8. トラブルシューティング

### ログイン失敗

**症状**: `401 Unauthorized` エラー

**原因**: ユーザー名またはパスワードが間違っている

**解決方法**: パスワードをリセット（管理者権限が必要）

```bash
curl -X PUT http://localhost:8080/api/users/{user_id} \
  -H "Authorization: Bearer $ADMIN_JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"password": "new_password"}'
```

### APIキーが表示されない

**症状**: APIキー一覧に名前しか表示されない

**原因**: セキュリティのため、キー本体は発行時のみ表示される

**解決方法**: 既存のキーを削除し、新しいキーを発行

### 全管理者ユーザーを削除してしまった

**症状**: ログインできなくなった

**解決方法**: データベースを直接編集、または再初期化

```bash
# データベースをバックアップ
cp ~/.llm-router/coordinator.db ~/.llm-router/coordinator.db.backup

# データベースを削除して再初期化
rm ~/.llm-router/coordinator.db

# コーディネーター再起動（新しい管理者作成）
cargo run --bin coordinator
```

## 9. セキュリティベストプラクティス

1. **強力なパスワード**: 最低8文字、大文字・小文字・数字・記号を組み合わせる
2. **APIキーの安全な保管**: 環境変数や専用のシークレット管理ツールに保存
3. **HTTPSの使用**: 本番環境ではリバースプロキシ（Nginx/Caddy）でHTTPS終端
4. **APIキーの定期的なローテーション**: 漏洩リスクを最小化
5. **認証無効化モードは信頼できるネットワークのみ**: パブリックネットワークでは使用しない

## 10. 次のステップ

- [ ] 複数のAPIキーを発行してアプリケーションごとに管理
- [ ] 閲覧専用ユーザーを作成してチームメンバーに配布
- [ ] APIキーに有効期限を設定して自動失効
- [ ] リバースプロキシでHTTPS設定（Let's Encrypt推奨）

---

**テスト完了基準**:
- [ ] 管理者作成が成功
- [ ] ダッシュボードログインが成功
- [ ] APIキー発行が成功
- [ ] 外部アプリケーションからAPIアクセスが成功
- [ ] 無効なAPIキーでのアクセスが拒否される
- [ ] エージェント登録とトークン使用が成功
- [ ] 認証無効化モードが動作する
