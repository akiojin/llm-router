# LLM runtimeモデルストレージ形式サポート

## 概要

C++ NodeのLlamaManagerがLLM runtimeのネイティブモデルストレージ形式（blobファイル）を
正しく認識・ロードできるようにする。

## ビジネス価値

- LLM runtimeでプルしたモデルをそのまま使用できる
- ユーザーが手動でモデルファイルを変換・移動する必要がない
- LLM runtimeエコシステムとのシームレスな統合

## ユーザーストーリー

### US-1: LLM runtimeでプルしたモデルを使用したい

ユーザーとして、`runtime pull`でダウンロードしたモデルを
C++ Nodeでそのまま使用できる。

**受け入れ条件**:

- LLM runtimeのblobストレージ形式（`~/.runtime/models/blobs/sha256-<hash>`）を認識する
- `.gguf`拡張子のファイルも引き続きサポートする
- manifestファイルからblobパスを正しく解決する

## 機能要件

### FR-1: モデルファイル形式

以下のモデルファイル形式をサポートする:

1. **GGUFファイル**: `.gguf`拡張子を持つファイル
2. **LLM runtime blobファイル**: `sha256-<64文字の16進数>`形式のファイル名

### FR-2: LLM runtimeストレージ構造

LLM runtimeの標準ストレージ構造を理解し、以下のパスを解決できる:

```text
~/.runtime/models/
├── manifests/
│   └── registry.runtime.ai/
│       └── library/
│           └── <model>/
│               └── <tag>    # JSON manifest
└── blobs/
    └── sha256-<hash>        # 実際のモデルファイル
```

### FR-3: Manifest解析

LLM runtimeのmanifestファイル（JSON形式）を解析し、
`application/vnd.runtime.image.model`タイプのレイヤーからblobパスを取得する。

### FR-4: Digestフォーマット変換

manifestの`digest`フィールド（`sha256:xxxx`形式）を
blobファイル名（`sha256-xxxx`形式）に変換する。

## 非機能要件

### NFR-1: 後方互換性

- 既存の`.gguf`ファイルロードは引き続き動作する
- 環境変数やAPIの変更なし

### NFR-2: エラーメッセージ

- 無効なファイル形式の場合、明確なエラーメッセージを表示する
- blobファイルが見つからない場合、manifestの内容を含むエラーを表示する

## テスト要件

### TDD-1: isLLM runtimeBlobFile関数テスト

- 有効なLLM runtime blobファイル名を正しく判定する
- 無効なファイル名を拒否する
- 境界ケース（空文字、短すぎる文字列など）を処理する

### TDD-2: loadModel関数テスト

- `.gguf`ファイルをロードできる
- LLM runtime blobファイルをロードできる
- 無効な形式を拒否する

### TDD-3: resolveModelPath関数テスト

- モデル名からblobパスを正しく解決する
- manifestが存在しない場合のエラー処理
- 無効なmanifest形式のエラー処理
