# gptossアーキテクチャエイリアスサポート タスク一覧

## Setup

- [x] SPEC-8a2d1d43ディレクトリ作成
- [x] spec.md作成
- [x] tasks.md作成

## Core

- [x] llama-arch.cppのllm_arch_from_string関数にgptossエイリアス追加
- [x] LLM_ARCH_NAMESを"gptoss"に変更（ハイパーパラメータキー名対応）
- [x] gpt-ossエイリアスの後方互換性対応
- [x] expert_feed_forward_lengthをオプショナル化（feed_forward_lengthへフォールバック）

## Polish

- [x] 品質チェック実行
- [x] コミット・プッシュ
