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
- `:latest` のような **可変タグは alias に含めない**。世代交代でルックアップ先が
  ねじれて、後方互換性を壊すため（例: `gemma4:latest` を撤廃）。
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

## 量子化サフィックス方針（G-3、暫定）

現状、モデル ID には量子化サフィックス（`:Q4_K_M`、`:Q5_K_M`、`:Q8_0` 等）が
含まれるケースと含まれないケースが混在する。本書執筆時点では**現状を維持**し、
構造的な分離（`id` と `quantization` フィールドへの分割）は別 SPEC で扱う。

**暫定運用**:

- `BUILTIN_MAPPINGS` に登録する canonical は **量子化サフィックス無し** の repo ID
  を優先する（例: `ggml-org/gemma-4-E4B-it-GGUF`）。
- 同一 repo の異なる量子化を別エントリで列挙することは、可能な限り避ける。
- エンドポイントが量子化サフィックス付き ID を申告してきた場合、self-canonical
  fallback により `canonical_name` には ID 自身が入る（mapping 不在のため）。
  恒久対応は `quantization` フィールド分離 SPEC を待つ。

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
- G-1: ggml-org gemma-4-E2B/E4B の canonical 追加（Gemma 4 系 SKU の実在性確認 G-5 が前提）
- G-3: 量子化サフィックス命名規則の構造変更（`id` ↔ `quantization` 分離は API 互換性影響あり）
- G-5: Gemma 4 SKU 実在性の事実確認（外部リソース照合）
- C-1 / C-2 / G-6: 単一エンドポイントの大量モデル誤申告の根本対応（エンドポイント側設定）
- C-3: モデル名妥当性の事実確認（外部リソース照合）

## 関連リンク

- 設計概要: [`architecture.md`](./architecture.md)
- mapping 実装: `llmlb/src/models/mapping.rs`
- /v1/models 実装: `llmlb/src/api/openai.rs` (`list_models`, `get_model`)
- 関連 Issue: [#575 OpenAI互換APIゲートウェイ](https://github.com/akiojin/llmlb/issues/575)
