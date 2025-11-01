# GPU Detection PoC

このPoCは、Ollama CoordinatorのAgentが起動時にGPUを自動検出するための方法を検証します。

## 目的

`ollama ps` コマンドは、モデルが実行中でない場合にGPU情報を返さないため、Agent起動時のGPU検出には使用できません。
このPoCでは、Ollamaの実装を参考に、デバイスファイルやsysfs、システムコマンドを使用した代替検出方法を検証します。

## 検出方法

### NVIDIA GPU

1. **デバイスファイル確認** (`/dev/nvidia0`, `/dev/nvidiactl` など)
   - NVIDIAドライバがインストールされている場合に存在
   - Docker環境でも `--gpus` オプション使用時に利用可能

2. **ドライババージョンファイル** (`/proc/driver/nvidia/version`)
   - NVIDIAドライバのバージョン情報
   - 存在すればNVIDIA GPUが利用可能

3. **ライブラリ検索** (`libnvidia-ml.so*`)
   - Ollamaと同様の検索パターンを使用
   - 複数の標準パスを確認

### AMD GPU

1. **KFDデバイスファイル** (`/dev/kfd`)
   - AMD ROCm Kernel Fusion Driver
   - ROCmインストール時に存在

2. **KFD Topology (sysfs)** (`/sys/class/kfd/kfd/topology/nodes/*/properties`)
   - Ollama v0.1.29+ で使用されている方法
   - `vendor_id 0x1002` でAMD GPUを識別
   - ライブラリ不要で動作

3. **DRMデバイス** (`/sys/class/drm/card*/device/vendor`)
   - Direct Rendering Managerインターフェース
   - `0x1002` でAMD GPUを識別

4. **DRIレンダーデバイス** (`/dev/dri/renderD128`-`/dev/dri/renderD135`)
   - GPU計算用のデバイスファイル

### Apple Silicon

1. **lscpuコマンド**
   - 出力に "Vendor ID: Apple" が含まれるかチェック
   - Docker for Mac環境でも動作

2. **/proc/cpuinfo**
   - `CPU implementer : 0x61` (Apple)
   - ARM64アーキテクチャでApple Siliconを識別

3. **sysctl (macOSネイティブのみ)**
   - `machdep.cpu.brand_string` でCPUブランド取得
   - `hw.perflevel0.physicalcpu` でパフォーマンスコア数取得
   - Docker内では動作しない

## ビルドと実行

```bash
cd poc/gpu-detection
cargo build --release
cargo run --release
```

## 検証結果

### テスト環境

- OS: Linux (Docker for Mac on Apple Silicon)
- Architecture: aarch64
- Docker: Yes (Docker for Mac)
- GPU: Apple Silicon (via Rosetta 2 / ARM64 emulation)

### 検出結果

- NVIDIA: [x] 検出失敗 (該当GPUなし - 想定通り)
- AMD: [x] 検出失敗 (該当GPUなし - 想定通り)
- Apple Silicon: [x] **検出成功！**

### 有効だった検出方法

**Apple Silicon検出 (成功)**:

1. **lscpu** ✓ 成功
   - `lscpu` コマンドの出力に "Vendor ID: Apple" が含まれることを確認
   - Docker for Mac環境でも正常に動作

2. **/proc/cpuinfo** ✓ 成功
   - `CPU implementer : 0x61` (Apple) を検出
   - すべてのCPUコアでApple implementerを確認

3. **sysctl** - スキップ
   - macOSネイティブでのみ動作
   - Docker内では利用不可（想定通り）

**結論**:
Docker for Mac環境でも、`lscpu` と `/proc/cpuinfo` を使用してApple Siliconを確実に検出できることが実証されました。
環境変数の設定は不要です。

## 統合方針

このPoCで有効性が確認できた検出方法を、`agent/src/metrics.rs` の `GpuCollector::detect_gpu()` に統合します。

### 変更内容

1. `OllamaPsGpuCollector` を削除
2. PoCで検証した検出ロジックを各GPUコレクタに実装
3. 検出優先順位:
   - NVIDIA: デバイスファイル → driver version → ライブラリ検索
   - AMD: sysfs (KFD topology) → KFDデバイスファイル → DRM
   - Apple Silicon: lscpu → /proc/cpuinfo → sysctl (macOSネイティブ)
4. 環境変数fallbackは最終手段として維持

## 参考

- [Ollama GPU detection implementation](https://github.com/ollama/ollama)
- NVIDIA: `/gpu/gpu_linux.go`
- AMD: `/gpu/amd_linux.go` (v0.1.29+)
- Apple Silicon: `/gpu/cpu_common.go`
