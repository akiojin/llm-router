# モデルカタログ運用ポリシー

`/v1/models` レスポンスにおける `id` / `canonical_name` / `aliases` / `max_tokens` /
`supported_apis` 等の挙動と、`llmlb/src/models/mapping.rs` のメンテナンス指針を定める。

本書は SPEC #575 「OpenAI互換APIゲートウェイ」US-001 の補正対応で発見された
メタデータ品質課題（B-1〜B-5、G-1〜G-7）を踏まえた運用方針である。

## エイリアス命名規則

`BUILTIN_MAPPINGS` (`llmlb/src/models/mapping.rs`) の `aliases` には、各エンドポイント
タイプ（Ollama / LM Studio / xLLM など）が利用するモデル名をすべて列挙する。

ルール:

- canonical 名は HuggingFace 上で**実在するリポジトリ ID** を採用する（例:
  `zai-org/GLM-4.7-Flash`、`openai/gpt-oss-20b`）。組織名が変わったモデルでは、
  旧 org の表記を alias として残す（例: `THUDM/glm-4.7-flash` → alias）。
- alias 名は **各エンドポイントタイプ固有の命名形式** に従う。
  - Ollama: `family:tag`（例: `gpt-oss:20b`、`gemma4:e4b`）
  - LM Studio: HuggingFace 形式（例: `openai/gpt-oss-20b`）
- 新規 mapping では `:latest` のような **可変タグを原則登録しない**。Issue #643 の
  Gemma 4 mapping には `gemma4:latest` を登録せず、SKU 固定タグを使う。一方、
  Qwen / GLM / Nomic 等の既存 `:latest` alias は後方互換性のため残り得る。
- 量子化サフィックス（`:Q4_K_M` など）は現状 ID に同居しているケースがあるが、
  正規化方針は別 SPEC で扱う（後述「量子化サフィックス方針」）。

## canonical_name の解決とフォールバック

`CanonicalResolution::canonical_for(model_key)` は次の順で解決する。

1. `model_key` 自身が canonical テーブルに登録 → そのまま返す。
2. `model_to_canonical` 逆引きで対応する canonical を返す。
3. いずれも未登録 → **`model_key` 自身を canonical として返す**（self-fallback）。

「mapping に登録があるか？」の判定が必要な場合は `CanonicalResolution::is_known()`
を併用する。`canonical_for` は `String` を必ず返す（`null` を返さない）。

## max_tokens フォールバック（B-1 / G-7）

`/v1/models` の `max_tokens` は次の優先順で解決する（`llmlb/src/models/mapping.rs:resolve_max_tokens`）。

1. エンドポイント側 `/v1/models` が申告した `max_tokens`。
2. `KNOWN_CONTEXT_LENGTHS` テーブルから canonical 一致で取得。
3. いずれにも該当なし → `null`。

`KNOWN_CONTEXT_LENGTHS` には公開情報（HuggingFace モデルカード等）から確認できた
モデルの context length のみを登録する。バリエーションごとに異なる値が公称される
モデル（量子化バリエーションで差がある等）は登録しない。

Issue #643 で確認した fallback 値は次のとおり。

| canonical | context length (tokens) |
|---|---:|
| `google/gemma-4-E2B-it` | 131072 |
| `google/gemma-4-E4B-it` | 131072 |
| `google/gemma-4-26B-A4B` | 262144 |
| `google/gemma-4-26B-A4B-it` | 262144 |
| `nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16` | 1048576 |
| `nvidia/NVIDIA-Nemotron-3-Nano-4B-BF16` | 262144 |
| `Qwen/Qwen3-Coder-Next` | 262144 |
| `zai-org/GLM-4.7-Flash` | 202752 |

Nemotron 30B の 1048576 はモデルカードの公称最大 1M tokens であり、標準設定の値と
異なる場合がある。実際の応答では前述のとおりエンドポイント申告値を優先する。

新しいモデルを追加する際は出典（HF README / 公式リリースノート等）を PR 説明に
記載する。

## 検証済み SKU と派生バリエーション（G-1 / G-2 / G-5 / C-3）

ggml-org のように community 配布版のリポジトリは、独自 SKU 接頭辞（`E2B`、`E4B`
等）を付ける場合がある。Issue #643 では一次情報と照合し、同一 SKU の GGUF
再配布を次の official canonical へ明示的に集約した。サイズ違い、および base と
instruction-tuned（`-it`）は別 SKU として扱う。

| official canonical | 検証済みの対応・備考 |
|---|---|
| `google/gemma-4-E2B-it` | `ggml-org/gemma-4-E2B-it-GGUF` |
| `google/gemma-4-E4B-it` | `ggml-org/gemma-4-E4B-it-GGUF` |
| `google/gemma-4-26B-A4B` | Google 公式 base。対応する Ollama alias は割り当てない |
| `google/gemma-4-26B-A4B-it` | `ggml-org/gemma-4-26B-A4B-it-GGUF` |
| `nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16` | NVIDIA 公式 30B-A3B BF16 |
| `nvidia/NVIDIA-Nemotron-3-Nano-4B-BF16` | NVIDIA 公式 4B BF16 |
| `Qwen/Qwen3-Coder-Next` | Qwen 公式 |
| `zai-org/GLM-4.7-Flash` | Z.ai 公式。旧 `THUDM/glm-4.7-flash` は alias |

Gemma 4 の Ollama runtime 名は、SKU を一意にする固定タグだけを利用する。

- E2B instruction-tuned: `gemma4:e2b`
- E4B instruction-tuned: outbound は固定タグ `gemma4:e4b` を優先する。既存の
  bare `gemma4` は互換入力専用の legacy alias として受理する
- 26B instruction-tuned: `gemma4:26b`
- 26B base: Ollama alias なし

`gemma4:latest` は参照先が将来変わり得るため、alias に含めない。

## 量子化サフィックス方針（G-3、暫定実装済み）

モデル ID には量子化サフィックス（`:Q4_K_M`、`:Q5_K_M`、`:Q8_0`、`:F16`、
`:IQ4_XS` 等）が含まれるケースと含まれないケースが混在する。後方互換性のため
**ID 自体は変更せず**、`/v1/models` レスポンスに新フィールド `quantization` を
追加して情報を別出しする方針を採用する。

### 実装（`llmlb/src/models/mapping.rs`）

- `split_quantization_suffix(model_id) -> (&str, Option<&str>)`:
  - `:Q[0-9]*`、`:IQ[0-9]*`、`:F16` / `:F32` / `:BF16` / `:FP16` / `:FP32` /
    `:F8E4M3FN` / `:F8E5M2` のいずれかにマッチした場合のみ suffix を分離。
  - Ollama 形式タグ（`:30b`、`:latest` 等）は誤検出しない。
- `CanonicalResolution::canonical_for(model_key)`:
  - 通常の lookup → 量子化サフィックスを除去した base での再 lookup → self-fallback
    の順で解決。これにより `ggml-org/repo:Q4_K_M` も `BUILTIN_MAPPINGS` に登録した
    base 名（`ggml-org/repo`）から canonical を引ける。

### レスポンスフィールド

```text
{
  "id": "ggml-org/gemma-4-E4B-it-GGUF:Q4_K_M",   // 互換のため変更しない
  "quantization": "Q4_K_M",                       // 新フィールド（追加）
  ...
}
```

量子化サフィックスを持たないモデルは `quantization: null`。

### 構造的分離（後方互換性影響あり）について

「`id` を量子化なしへ正規化し、エンドポイントへのルーティング時に `quantization`
を別経路で扱う」構造的分離はクライアント・dashboard・OpenAI 互換契約への影響が
大きい。SPEC #575 US-029 では requestable な detail ID を維持し、canonical と
variant の集約情報を別に提供する互換方針を採用したため、G-3-struct の破壊的な ID
正規化案は superseded とする。

## 異常検知ログ（C-defensive）

`EndpointRegistry::sync_models()` は単一エンドポイントが
`SUSPICIOUS_MODEL_COUNT_THRESHOLD`（既定 50）件超のモデルを申告した場合に
`tracing::warn!()` を出力する。これは「カタログ集約サーバが実体を持たない全モデルを
`/v1/models` で返してしまう」誤申告（C-1 / C-2 / G-6 で観測）を運用者が早期に
気付くための観測ポイント。

## `lifecycle_status` / `download_progress` の扱い（B-4）

両フィールドは現状ダッシュボード UI（`ModelsTable.tsx`）が消費している。値は
ほぼ常に `Registered` / `null` だが、UI 連動のため API レスポンスからは削除しない。
状態管理を実装するか、フィールドそのものを廃止するかは別 SPEC で扱う。

## Issue #643 の対応状況と残課題

- G-1 / G-5 / C-3: Issue #643 で公式モデル ID、派生関係、runtime alias、context
  fallback を一次情報と照合し、`BUILTIN_MAPPINGS` と本書へ反映済み。
- G-3-struct: SPEC #575 US-029 の互換方針により superseded。requestable な `id` は
  維持し、canonical / variant 情報で集約する。
- B-4: `lifecycle_status` / `download_progress` は UI/API のプロダクト判断が必要なため、
  lifecycle 状態管理の実装またはフィールド廃止を別 owner/decision として扱う。
- C-1 / C-2 / G-6: 単一エンドポイントの大量モデル誤申告は operator / endpoint 側の
  責務として扱う。llmlb 側は `sync_models()` の警告ログで早期検知する。

## 関連リンク

- 設計概要: [`architecture.md`](./architecture.md)
- mapping 実装: `llmlb/src/models/mapping.rs`
- /v1/models 実装: `llmlb/src/api/openai.rs` (`list_models`, `get_model`)
- 親 SPEC: [#575 OpenAI互換APIゲートウェイ](https://github.com/akiojin/llmlb/issues/575)
- Follow-up Issue: [#643 /v1/models 品質改善: 残課題](https://github.com/akiojin/llmlb/issues/643)
