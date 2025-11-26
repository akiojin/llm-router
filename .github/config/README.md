# Repository Configuration

このディレクトリにはGitHubリポジトリの設定をバージョン管理するためのファイルが含まれています。

## ファイル構成

```text
.github/config/
├── README.md                      # このファイル
├── repo.json                      # リポジトリ設定
└── branch-protection/
    ├── main.json                  # mainブランチの保護ルール
    └── develop.json               # developブランチの保護ルール
```

## 設定の適用

[gh-repo-config](https://github.com/twelvelabs/gh-repo-config) を使用して設定を適用します。

### インストール

```bash
gh extension install twelvelabs/gh-repo-config
```

### 設定の適用

```bash
gh repo-config apply
```

## ブランチ保護ルール

### main

- **PR必須**: 直接プッシュ禁止
- **Required Checks**: なし（developで検証済みのため）
- **承認**: 0人（自動マージ用）

### develop

- **Required Checks**: 全てのCIチェック必須
  - Commit Message Lint
  - Markdown Lint
  - Rust Format & Clippy
  - Rust Tests (ubuntu-latest)
  - Rust Tests (windows-latest)

## リリースフロー

```text
feature → develop (PR + CI必須)
    ↓
/release 実行
    ↓
develop → main (PR自動マージ)
    ↓
release-please → リリースPR
    ↓
タグ作成 → 配布
```
