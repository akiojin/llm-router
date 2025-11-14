# macOS SDK セットアップガイド

このドキュメントでは、Dockerイメージでのmacosクロスコンパイルに必要なmacOS SDKの
取得と準備方法を説明します。

## 概要

ollama-coordinatorのmacOS向けバイナリをLinux環境（Docker）でビルドするには、
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
# ollama-coordinatorプロジェクトのルートディレクトリに移動
cd /path/to/ollama-coordinator

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

## 参考リンク

- [osxcross - macOS クロスコンパイルツールチェーン](https://github.com/tpoechtrager/osxcross)
- [Apple Developer Documentation](https://developer.apple.com/documentation/)
- [Xcode Downloads](https://developer.apple.com/download/)
