# Ollamaモデルストレージ形式サポート

## 概要

C++ NodeのLlamaManagerがOllamaのネイティブモデルストレージ形式（blobファイル）を
正しく認識・ロードできるようにする。

## ビジネス価値

- Ollamaでプルしたモデルをそのまま使用できる
- ユーザーが手動でモデルファイルを変換・移動する必要がない
- Ollamaエコシステムとのシームレスな統合

## ユーザーストーリー

### US-1: Ollamaでプルしたモデルを使用したい

ユーザーとして、`ollama pull`でダウンロードしたモデルを
C++ Nodeでそのまま使用できる。

**受け入れ条件**:

- Ollamaのblobストレージ形式（`~/.ollama/models/blobs/sha256-<hash>`）を認識する
- `.gguf`拡張子のファイルも引き続きサポートする
- manifestファイルからblobパスを正しく解決する

## 機能要件

### FR-1: モデルファイル形式

以下のモデルファイル形式をサポートする:

1. **GGUFファイル**: `.gguf`拡張子を持つファイル
2. **Ollama blobファイル**: `sha256-<64文字の16進数>`形式のファイル名

### FR-2: Ollamaストレージ構造

Ollamaの標準ストレージ構造を理解し、以下のパスを解決できる:

```text
~/.ollama/models/
├── manifests/
│   └── registry.ollama.ai/
│       └── library/
│           └── <model>/
│               └── <tag>    # JSON manifest
└── blobs/
    └── sha256-<hash>        # 実際のモデルファイル
```

### FR-3: Manifest解析

Ollamaのmanifestファイル（JSON形式）を解析し、
`application/vnd.ollama.image.model`タイプのレイヤーからblobパスを取得する。

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

### TDD-1: isOllamaBlobFile関数テスト

- 有効なOllama blobファイル名を正しく判定する
- 無効なファイル名を拒否する
- 境界ケース（空文字、短すぎる文字列など）を処理する

### TDD-2: loadModel関数テスト

- `.gguf`ファイルをロードできる
- Ollama blobファイルをロードできる
- 無効な形式を拒否する

### TDD-3: resolveModelPath関数テスト

- モデル名からblobパスを正しく解決する
- manifestが存在しない場合のエラー処理
- 無効なmanifest形式のエラー処理
