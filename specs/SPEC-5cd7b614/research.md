# GPU検出の技術調査結果

## 調査日

2025-11-01

## 調査目的

Ollama CoordinatorのAgentが起動時にGPUを自動検出する方法を確立する。
当初実装した `ollama ps` ベースの検出が、モデル非実行時にはGPU情報を返さないことが判明したため、代替手段を検証する。

## 問題の発見

### ollama psコマンドの制限

`ollama ps` コマンドは、モデルが実行中の場合のみPROCESSOR列にGPU情報を表示する。
Agent起動時にはモデルが実行されていないため、以下のように空の出力が返される：

```
NAME    ID    SIZE    PROCESSOR    CONTEXT    UNTIL

```

この制限により、`ollama ps` ベースのGPU検出は起動時の自動検出に使用できないことが確認された。

### 環境変数アプローチの却下

環境変数 (`OLLAMA_GPU_AVAILABLE`, `OLLAMA_GPU_MODEL` など) を必須とするアプローチは、ユーザーの設定負担が大きいため却下された。

## Ollama実装の調査

Ollamaのソースコードを参照し、各GPUベンダーの検出方法を調査した。

### NVIDIA GPU検出

Ollamaの実装 (`/gpu/gpu_linux.go`):

1. **ライブラリ検索**: 複数のパスで `libnvidia-ml.so*` を探索
   ```
   /usr/local/cuda/lib64/libnvidia-ml.so*
   /usr/lib/x86_64-linux-gnu/nvidia/current/libnvidia-ml.so*
   /usr/lib/x86_64-linux-gnu/libnvidia-ml.so*
   /usr/lib/wsl/lib/libnvidia-ml.so*
   /opt/cuda/lib64/libnvidia-ml.so*
   ```

2. **デバイスファイル**: `/dev/nvidia0`, `/dev/nvidiactl` の存在確認

3. **ドライババージョン**: `/proc/driver/nvidia/version` からバージョン情報を取得

### AMD GPU検出

Ollamaの実装 (`/gpu/amd_linux.go`, v0.1.29+):

1. **sysfs KFD Topology** (推奨方法):
   - `/sys/class/kfd/kfd/topology/nodes/*/properties` を読み取り
   - `vendor_id 0x1002` (AMD) を確認
   - **ライブラリ不要**で動作

2. **KFDデバイスファイル**: `/dev/kfd` の存在確認

3. **DRMデバイス**: `/sys/class/drm/card*/device/vendor` で `0x1002` を確認

4. **DRIレンダーデバイス**: `/dev/dri/renderD*` の存在確認

### Apple Silicon検出

Ollamaの実装 (`/gpu/cpu_common.go`):

1. **sysctl (macOSネイティブ)**:
   - `hw.perflevel0.physicalcpu` でパフォーマンスコア数を取得
   - `machdep.cpu.brand_string` でCPUブランドを取得

2. **lscpu / /proc/cpuinfo (Linux/Docker)**:
   - lscpu出力の "Vendor ID: Apple"
   - /proc/cpuinfoの "CPU implementer : 0x61" (Apple)

## PoCの実装と検証

### PoC構成

`poc/gpu-detection/` に以下のスタンドアロンRustプロジェクトを作成：

- `src/nvidia.rs`: NVIDIA GPU検出ロジック
- `src/amd.rs`: AMD GPU検出ロジック
- `src/apple.rs`: Apple Silicon検出ロジック
- `src/main.rs`: 統合エントリーポイント

### 検証環境

- **OS**: Linux (Docker for Mac on Apple Silicon)
- **アーキテクチャ**: aarch64
- **Docker**: Docker for Mac
- **ホストGPU**: Apple Silicon (M系チップ)

### 検証結果

#### NVIDIA GPU

**結果**: 検出されず（該当GPUなし - 想定通り）

検証した検出方法:
- ✗ `/dev/nvidia0` - 存在しない
- ✗ `/proc/driver/nvidia/version` - 存在しない
- ✗ `libnvidia-ml.so*` - すべてのパスで未発見

#### AMD GPU

**結果**: 検出されず（該当GPUなし - 想定通り）

検証した検出方法:
- ✗ `/dev/kfd` - 存在しない
- ✗ `/sys/class/kfd/kfd/topology/nodes` - 存在しない
- ✗ `/sys/class/drm/card*/device/vendor` - 0x1002 未検出
- ✗ `/dev/dri/renderD*` - 存在しない

#### Apple Silicon

**結果**: ✓ **検出成功！**

検証した検出方法:

1. **lscpu** ✓ **成功**
   ```
   Vendor ID: Apple
   Architecture: aarch64
   ```
   Docker for Mac環境でも正常に動作

2. **/proc/cpuinfo** ✓ **成功**
   ```
   CPU implementer : 0x61
   ```
   すべてのCPUコアでApple implementer (0x61) を確認

3. **sysctl** - スキップ
   macOSネイティブでのみ動作（Docker内では利用不可）

### 重要な発見

1. **Docker for Mac環境でのApple Silicon検出が可能**
   - `lscpu` と `/proc/cpuinfo` を使用すれば、環境変数なしで自動検出できる
   - Dockerコンテナ内でも正常に動作

2. **環境変数が不要**
   - ユーザーの設定作業を削減
   - Docker環境でも自動的にGPUを認識

3. **Ollamaの実装パターンが有効**
   - sysfsベースの検出（AMD）
   - デバイスファイルベースの検出（NVIDIA）
   - システムコマンド/proc読み取り（Apple Silicon）

## 統合方針

### 変更対象

`agent/src/metrics.rs` の `GpuCollector` 実装

### 変更内容

1. **OllamaPsGpuCollectorの削除**
   - `ollama ps` ベースの検出を完全に削除
   - 関連するテストも削除

2. **各GPUコレクタの強化**

   **NvidiaGpuCollector**:
   - 優先順位1: `/dev/nvidia0` デバイスファイル確認
   - 優先順位2: `/proc/driver/nvidia/version` 読み取り
   - 優先順位3: `libnvidia-ml.so*` ライブラリ検索

   **AmdGpuCollector** (新規追加):
   - 優先順位1: `/sys/class/kfd/kfd/topology/nodes/*/properties` でvendor_id確認
   - 優先順位2: `/dev/kfd` デバイスファイル確認
   - 優先順位3: `/sys/class/drm/card*/device/vendor` で0x1002確認

   **AppleSiliconGpuCollector**:
   - 優先順位1: `lscpu` で "Vendor ID: Apple" 確認
   - 優先順位2: `/proc/cpuinfo` で "CPU implementer : 0x61" 確認
   - 優先順位3: `sysctl` (macOSネイティブのみ)

3. **環境変数fallback**
   - `OLLAMA_GPU_AVAILABLE`, `OLLAMA_GPU_MODEL`, `OLLAMA_GPU_COUNT`
   - 最終手段として維持（検出失敗時のみ）

4. **検出順序**
   - NVIDIA → AMD → Apple Silicon → 環境変数
   - 最初に検出された方法を使用

### テスト戦略

1. **単体テスト**
   - 各検出メソッドの個別テスト
   - デバイスファイル存在/非存在のモックテスト

2. **統合テスト**
   - Docker環境での実際の検出テスト
   - 各GPUベンダーでの動作確認

## 2025-11-02 追加検証ログ

- `agent/src/metrics.rs` にテスト専用の環境変数オーバーライド（`OLLAMA_TEST_*` 系）を追加し、Docker for Mac を想定した `lscpu` / `/proc/cpuinfo` のモック出力で Apple Silicon を判定できることをユニットテストで検証。
- AMD GPU については KFD topology / `/dev/kfd` / `/sys/class/drm` を一時ディレクトリで再現し、`AmdGpuCollector::new()` が1台検出することを確認。
- NVIDIA GPU は `/dev/nvidia0` と `/proc/driver/nvidia/version` のモックファイルで `is_nvidia_gpu_present()` が真を返すテストを追加。
- これらのテストにより CI 環境（GPU非搭載）でも検出ロジックを安全に回帰テストできるようになった。

1. **E2Eテスト**
   - Agent起動時の自動登録フロー
   - GPU情報がCoordinatorに正しく送信されることを確認

## 参考資料

- [Ollama GitHub Repository](https://github.com/ollama/ollama)
  - `gpu/gpu_linux.go` (NVIDIA)
  - `gpu/amd_linux.go` (AMD)
  - `gpu/cpu_common.go` (Apple Silicon)
- [NVIDIA Management Library (NVML)](https://developer.nvidia.com/nvidia-management-library-nvml)
- [AMD ROCm Documentation](https://rocmdocs.amd.com/)
- [Linux Kernel DRM Documentation](https://www.kernel.org/doc/html/latest/gpu/index.html)

## 次のステップ

1. `agent/src/metrics.rs` から `OllamaPsGpuCollector` を削除
2. PoCで検証した検出ロジックを各GPUコレクタに統合
3. テストの更新と実行
4. ドキュメントの更新
5. PRの作成とマージ
