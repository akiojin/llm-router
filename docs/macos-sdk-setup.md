# macOS SDK セットアップガイド

このドキュメントでは、Dockerイメージでのmacosクロスコンパイルに必要なmacOS SDKの
取得と準備方法を説明します。

## 概要

llm-routerのmacOS向けバイナリをLinux環境（Docker）でビルドするには、
macOS SDKが必要です。このSDKはAppleのライセンス規約により、
各開発者が個別に取得する必要があります。

## 前提条件

- **macOSマシン**: SDK取得にはmacOSマシンへのアクセスが必要です
- **Xcode または Command Line Tools**: いずれかがインストールされている必要があります
- **ディスク空き容量**: 約2GB以上

## SDK取得手順

### Step 1: Xcode/Command Line Toolsのインストール

以下のいずれかの方法でインストールします：

**方法A: Xcode（推奨）**

```bash
# App StoreからXcodeをインストール
# または以下のコマンドでCommand Line Toolsをインストール
xcode-select --install
```

**方法B: Apple Developer サイト**

<https://developer.apple.com/download/all/> から
「Command Line Tools for Xcode」をダウンロードしてインストール

### Step 2: SDKの場所を確認

```bash
# Xcodeを使用している場合
ls /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/

# Command Line Toolsのみの場合
ls /Library/Developer/CommandLineTools/SDKs/
```

利用可能なSDKが表示されます（例: `MacOSX14.2.sdk`, `MacOSX14.5.sdk`）

### Step 3: SDKをtar.xz形式でパッケージング

```bash
# SDKのバージョンを確認（例: 14.2）
SDK_VERSION="14.2"

# Xcodeを使用している場合
SDK_PATH="/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX${SDK_VERSION}.sdk"

# Command Line Toolsのみの場合
# SDK_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX${SDK_VERSION}.sdk"

# 作業ディレクトリを作成
mkdir -p ~/sdk-packages

# SDKをtar.xz形式でパッケージング
cd /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/
tar -cJf ~/sdk-packages/MacOSX${SDK_VERSION}.sdk.tar.xz MacOSX${SDK_VERSION}.sdk

# 完了確認
ls -lh ~/sdk-packages/MacOSX${SDK_VERSION}.sdk.tar.xz
```

このコマンドは数分かかる場合があります。完了すると約300-500MBのファイルが作成されます。

### Step 4: SDKファイルをプロジェクトに配置

```bash
# llm-routerプロジェクトのルートディレクトリに移動
cd /path/to/llm-router

# .sdkディレクトリを作成
mkdir -p .sdk

# SDKファイルをコピー
cp ~/sdk-packages/MacOSX${SDK_VERSION}.sdk.tar.xz .sdk/

# 配置確認
ls -lh .sdk/
```

### Step 5: Docker buildコンテキストの準備

SDKファイルがプロジェクトルートの`.sdk/`ディレクトリに配置されていることを確認します：

```bash
tree -L 2 .sdk/
# .sdk/
# └── MacOSX14.2.sdk.tar.xz
```

## SDK バージョンについて

### 推奨バージョン

- **MacOSX14.x.sdk**: macOS 14 Sonoma SDK（推奨）
  - 最新のAPIとツールチェーンをサポート
  - Apple Silicon (aarch64) と Intel (x86_64) の両方に対応

### 互換性マトリクス

| SDK Version | macOS Version | 対応アーキテクチャ | 推奨度 |
|-------------|---------------|-------------------|--------|
| 14.x        | Sonoma        | x86_64, aarch64   | ⭐⭐⭐ |
| 13.x        | Ventura       | x86_64, aarch64   | ⭐⭐   |
| 12.x        | Monterey      | x86_64, aarch64   | ⭐     |
| 11.x        | Big Sur       | x86_64, aarch64   | △     |

## トラブルシューティング

### SDKディレクトリが見つからない

**症状**: `/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/` が存在しない

**解決方法**:

```bash
# Xcodeのパスを確認
xcode-select -p

# パスが正しくない場合は設定
sudo xcode-select --switch /Applications/Xcode.app

# または Command Line Tools を再インストール
sudo rm -rf /Library/Developer/CommandLineTools
xcode-select --install
```

### tar.xz作成に失敗する

**症状**: `tar: Error opening archive: Failed to open`

**解決方法**:

```bash
# xz コマンドがインストールされているか確認
which xz

# インストールされていない場合はHomebrewでインストール
brew install xz

# 再試行
tar -cJf ~/sdk-packages/MacOSX14.2.sdk.tar.xz MacOSX14.2.sdk
```

### 複数のSDKバージョンがある場合

```bash
# 利用可能なすべてのSDKを表示
ls /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/

# 最新のSDKを自動選択
LATEST_SDK=$(ls -1 /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/ | sort -V | tail -1)
echo "Latest SDK: $LATEST_SDK"

# パッケージング
tar -cJf ~/sdk-packages/${LATEST_SDK}.tar.xz $LATEST_SDK
```

## ライセンスとコンプライアンス

### 重要な注意事項

- **Appleライセンス規約**: macOS SDKはAppleの利用規約に従って使用してください
- **リポジトリへの含有禁止**: SDKファイルをGitリポジトリにコミットしないでください
- **個人使用**: SDKは開発目的でのみ使用してください
- **再配布禁止**: SDKファイルを第三者と共有しないでください

### .gitignoreの設定

SDKファイルがGitに追跡されないよう、`.gitignore`に以下が含まれていることを確認してください：

```gitignore
# macOS SDK files
.sdk/
*.sdk/
*.sdk.tar.xz
```

## 次のステップ

SDKの準備が完了したら、以下を実行してDockerイメージをビルドします：

```bash
# Dockerイメージのビルド
docker-compose build

# macOS向けバイナリのビルド（x86_64）
make build-macos-x86_64

# macOS向けバイナリのビルド（aarch64）
make build-macos-aarch64

# 両方のアーキテクチャをビルド
make build-macos-all
```

## macOSでのネイティブビルド

macOS環境で直接ビルドする場合、上記のSDKセットアップは不要です。
以下の手順でネイティブビルドが可能です。

### 前提条件

- macOS（Intel または Apple Silicon）
- Rust ツールチェーン（`rustup`でインストール推奨）
- Xcode Command Line Tools

### ビルド手順

#### 1. Command Line Toolsのインストール

```bash
xcode-select --install
```

#### 2. Rustのインストール（未インストールの場合）

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

#### 3. ビルド実行

```bash
# プロジェクトルートディレクトリで実行
cargo build --release

# または、特定のターゲットを指定
# Intel Mac向け
cargo build --release --target x86_64-apple-darwin

# Apple Silicon Mac向け
cargo build --release --target aarch64-apple-darwin
```

#### 4. ビルド成果物の確認

```bash
# デフォルトターゲット
ls -lh target/release/llm-router

# 特定ターゲット
ls -lh target/x86_64-apple-darwin/release/llm-router
ls -lh target/aarch64-apple-darwin/release/llm-router
```

### トラブルシューティング

#### リンカーエラーが発生する場合

**症状**: `error: linker 'aarch64-apple-darwin23-clang' not found`

**原因**: 古いバージョンの設定ファイルがクロスコンパイル用のリンカーを
指定している可能性があります。

**解決方法**:

最新のリポジトリをpullして、`.cargo/config.toml`が更新されていることを確認してください。
`.cargo/config.toml`にはmacOS向けのリンカー設定が含まれていないはずです。

#### ターゲット追加が必要な場合

```bash
# Intel Mac向けターゲット追加（Apple Silicon Macで必要な場合）
rustup target add x86_64-apple-darwin

# Apple Silicon向けターゲット追加（Intel Macで必要な場合）
rustup target add aarch64-apple-darwin
```

### クロスコンパイルとネイティブビルドの違い

| 項目 | ネイティブビルド | クロスコンパイル |
|------|------------------|------------------|
| 実行環境 | macOS | Linux (Docker) |
| SDK取得 | 不要（システムのXcodeを使用） | 必要（`.sdk/`に配置） |
| リンカー | システムのclang | osxcrossのclang |
| ビルド時間 | 速い | やや遅い |
| 用途 | ローカル開発・テスト | CI/CD、複数プラットフォーム対応 |

## 参考リンク

- [osxcross - macOS クロスコンパイルツールチェーン](https://github.com/tpoechtrager/osxcross)
- [Apple Developer Documentation](https://developer.apple.com/documentation/)
- [Xcode Downloads](https://developer.apple.com/download/)
- [Rustup - Rust インストーラー](https://rustup.rs/)
