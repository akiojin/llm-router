# 実装計画: 統一APIプロキシ

**機能ID**: `SPEC-63acef08` | **ステータス**: ✅ 実装済み (PR #1)
**依存SPEC**: SPEC-94621a1f

## 概要
Ollama API互換のプロキシエンドポイント（/api/chat, /api/generate）。Round-robin方式でエージェント選択。

## 技術コンテキスト
**言語**: Rust 1.75+
**主要依存関係**: Axum, reqwest, tokio
**実装**: `coordinator/src/api/proxy.rs`
**テスト**: cargo test

## 実装完了
- [x] Round-robin負荷分散（AtomicUsize）
- [x] POST /api/chat エンドポイント
- [x] POST /api/generate エンドポイント  
- [x] エラーハンドリング（503エラー: エージェントなし）

**実装PR**: #1 | **マージ日**: 2025-10-30
