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
  `zai-org/glm-4.7-flash`、`openai/gpt-oss-20b`）。組織名が変わったモデルでは、
  旧 org の表記を alias として残す（例: `THUDM/glm-4.7-flash` → alias）。
- alias 名は **各エンドポイントタイプ固有の命名形式** に従う。
  - Ollama: `family:tag`（例: `gpt-oss:20b`、`gemma4`）
  - LM Studio: HuggingFace 形式（例: `openai/gpt-oss-20b`）
- `:latest` のような **可変タグは優先 alias にしない**。世代交代でルックアップ先が
  ねじれて、後方互換性を壊すため（例: `gemma4` を優先し、`gemma4:latest` は
  runtime が返す既存 ID の canonical 正規化にだけ使う）。
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

新しいモデルを追加する際は出典（HF README / 公式リリースノート等）を PR 説明に
記載する。

## SKU 命名と派生バリエーション（G-2）

ggml-org のように community 配布版のリポジトリは、独自 SKU 接頭辞（`E2B`、`E4B`
等）を付ける場合がある。これらは公式の SKU と区別される派生で、自動的な canonical
集約はしない方針。必要なら個別に `BUILTIN_MAPPINGS` に登録する。

例:

- `ggml-org/gemma-4-E2B-it-GGUF`: ggml-org community エッジ派生（仮）
- `google/gemma-4-26b-a4b`: Google 公式の MoE 版（実在性は別途確認）

派生バリエーションは「同じ世代の別 SKU」として扱う。利用者には UI/ドキュメントで
派生関係を明示する（dashboard 側の表示改善は別 SPEC）。

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
大きいため、この PR では扱わず別 SPEC（要 Issue 起票）で扱う。

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

## 別 SPEC へ送る項目（このブランチでは扱わない）

- B-4: `lifecycle_status` / `download_progress` の挙動見直し（UI 連動の影響範囲が広い）
- G-1: ggml-org gemma-4-E2B/E4B の canonical 追加（Gemma 4 系 SKU の実在性確認 G-5 が前提。本書時点では self-canonical fallback で `canonical_name` に id 自身が入る暫定動作）
- G-3 構造分離: `id` から量子化部分を除去し別経路でルーティングする破壊的変更（本書時点では `quantization` フィールド追加で情報の別出しまで実施）
- G-5: Gemma 4 SKU 実在性の事実確認（外部リソース照合）
- C-1 / C-2 / G-6: 単一エンドポイントの大量モデル誤申告の根本対応（エンドポイント側設定。本書時点では sync_models() の警告ログで早期検知のみ）
- C-3: モデル名妥当性の事実確認（外部リソース照合）

## 関連リンク

- 設計概要: [`architecture.md`](./architecture.md)
- mapping 実装: `llmlb/src/models/mapping.rs`
- /v1/models 実装: `llmlb/src/api/openai.rs` (`list_models`, `get_model`)
- 親 SPEC: [#575 OpenAI互換APIゲートウェイ](https://github.com/akiojin/llmlb/issues/575)
- Follow-up Issue: [#643 /v1/models 品質改善: 残課題](https://github.com/akiojin/llmlb/issues/643)
