# gptossアーキテクチャエイリアスサポート

## 概要

llama.cppが`gptoss`アーキテクチャ名（ハイフンなし）を認識できるようにする。

## ビジネス価値

- LLM runtimeでプルしたgpt-ossモデルをC++ Nodeで使用できる
- ユーザーがモデル変換等の追加作業なしで推論を実行できる

## ユーザーストーリー

### US-1: gpt-ossモデルを使用したい

ユーザーとして、`runtime pull gpt-oss:20b`でダウンロードしたモデルを
C++ Nodeでそのまま使用できる。

**受け入れ条件**:

- GGUFメタデータの`general.architecture = "gptoss"`を認識する
- `LLM_ARCH_OPENAI_MOE`として処理される
- 既存の`gpt-oss`（ハイフン付き）も引き続きサポートする

## 機能要件

### FR-1: アーキテクチャ名エイリアス

`llm_arch_from_string`関数で以下のエイリアスをサポート:

| 入力 | 出力 |
|------|------|
| `gptoss` | `LLM_ARCH_OPENAI_MOE` |
| `gpt-oss` | `LLM_ARCH_OPENAI_MOE` |

## 非機能要件

### NFR-1: 後方互換性

- 既存の`gpt-oss`（ハイフン付き）は引き続き動作する
- 他のアーキテクチャへの影響なし

## 技術的背景

### 問題の原因

LLM runtimeが生成するGGUFファイルは以下の形式を使用する:

- アーキテクチャ名: `general.architecture = "gptoss"`
- ハイパーパラメータキー: `gptoss.context_length`, `gptoss.embedding_length`等

しかし、llama.cppのマッピングは`"gpt-oss"`（ハイフン付き）のみを認識していた。

```cpp
// llama-arch.cpp 99行目（修正前）
{ LLM_ARCH_OPENAI_MOE,       "gpt-oss"          },
```

これにより以下の2つの問題が発生:

1. `llm_arch_from_string("gptoss")`が`LLM_ARCH_UNKNOWN`を返す
2. ハイパーパラメータキー`gpt-oss.context_length`を探すが、
   GGUFには`gptoss.context_length`が格納されている

### 解決策

`LLM_ARCH_NAMES`のマッピングを`"gptoss"`に変更し、
`llm_arch_from_string`で`"gpt-oss"`のエイリアスも認識するようにした。

### llama.cpp本家との同期

テンソル定義やグラフビルダーをllama.cpp本家と同期:

- `LLM_TENSOR_ATTN_POST_NORM` テンソル追加
- `LLM_TENSOR_ATTN_SINKS` テンソル追加
- バイアステンソル（bq, bk, bv, bo, ffn_*_b）追加
- `llm_build_openai_moe_iswa` グラフビルダー使用
- SWAパターン設定の追加
