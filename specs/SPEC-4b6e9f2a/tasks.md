# タスク分解: クラウドモデルプレフィックスルーティング

**機能ID**: `SPEC-4b6e9f2a`  
**ステータス**: 未着手

## Setup

- [ ] 環境変数仕様を定義し README/USAGE に追記（OPENAI/GOOGLE/ANTHROPIC のキーとベースURL）
- [ ] クラウド用メトリクス項目の命名・ラベルを決定

## Tests (先行)

- [x] Unit: モデル名パーサ（プレフィックス検出、タイポ互換、デフォルトローカル）
- [x] Unit: 設定バリデーション（キー未設定時のエラー）
- [ ] Integration: クラウドプレフィックス付きリクエストがローカルノードに到達しないことをモックで検証
- [ ] Integration: OpenAI/Google/Anthropic 各モッククライアントの非ストリーミング応答をプロキシできること
- [ ] Integration: `stream: true` を指定した場合にSSEでチャンクが順次届くこと
- [ ] Regression: プレフィックスなしモデルが従来のローカルルートで動作し続けること

## Core Implementation

- [x] モデル名パーサとクラウドルート判定の共通化
- [x] OpenAI クラウドクライアント実装（非ストリーミング/ストリーミング）
- [x] Google クラウドクライアント実装（非ストリーミング/ストリーミング）
- [x] Anthropic クラウドクライアント実装（非ストリーミング/ストリーミング）
- [ ] エラーハンドリングとHTTPステータスの整理

## Integration / Observability

- [ ] ログに `provider`, `model`, `request_id`, `latency_ms` を出力
- [ ] メトリクスにベンダー別カウンタ・レイテンシヒストグラムを追加

## Docs

- [ ] README/USAGE/API仕様にプレフィックス利用方法・環境変数・制限事項を追記
- [ ] 変更点を CHANGELOG に反映
