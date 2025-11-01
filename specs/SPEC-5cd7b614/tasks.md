# 実装タスク: GPU必須エージェント登録要件

**SPEC ID**: SPEC-5cd7b614
**作成日**: 2025-11-01

## Phase 0: 技術リサーチ ✅

- [x] Ollamaのソースコードを調査してGPU検出方法を確認
- [x] PoCプロジェクトでGPU検出を検証
- [x] research.mdに調査結果を記録

## Phase 1: GPU検出機能の実装 ✅

### Agent側

- [x] AppleSiliconGpuCollectorの条件コンパイル制限を削除
- [x] lscpuによるApple Silicon検出をLinux環境で有効化
- [x] /proc/cpuinfoによるApple Silicon検出をLinux環境で有効化
- [x] Metal APIの使用部分のみmacOS専用に保護
- [x] GpuCollector enumの条件コンパイル修正
- [x] すべてのmatch文で条件コンパイル修正
- [x] AMD GPU検出機能の追加
- [x] NVIDIA GPU検出の事前チェック追加

### テスト

- [x] GPU検出の既存テストを確認
- [ ] Docker for Mac環境でのApple Silicon検出テストを追加
- [ ] AMD GPU検出テストを追加（モック使用）
- [ ] NVIDIA GPU検出テストを確認

## Phase 2: ダッシュボード表示の改善 ✅

### Coordinator側

- [x] app.jsのGPU表示ロジックを修正
- [x] gpu_availableとgpu_modelを確認してモデル名表示
- [x] テーブル表示: "GPU {モデル名}"
- [x] モーダル表示: "{モデル名} (メトリクス非対応)"
- [ ] ダッシュボード表示のE2Eテストを追加

### テスト

- [ ] Coordinator APIレスポンスのテストを追加
- [ ] GPU情報を含むエージェント登録のテストを確認

## Phase 3: ドキュメント更新 ⏳

- [ ] spec.mdにDocker for Mac対応を追記
- [ ] README.mdにGPU検出方法を記載
- [ ] research.mdの統合結果を記録

## Phase 4: 統合テストとリリース ⏳

- [x] 全体の動作確認（Docker for Mac環境）
- [ ] CIでのテスト成功を確認
- [ ] PRマージ後の動作確認

## 完了条件

- [x] Docker for Mac環境でApple Siliconが自動検出される
- [x] ダッシュボードに「GPU Apple Silicon」と表示される
- [ ] すべてのテストが成功する
- [ ] ドキュメントが更新される
