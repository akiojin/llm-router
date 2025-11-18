<!-- UI Snapshot Note: Tabs removed, single-view layout -->
Target: coordinator/src/web/static/index.html

- 要件: 上部のタブ（ダッシュボード／エージェント一覧／モデル管理／リクエスト履歴／ログ）を表示しないこと。
- 検証: 2025-11-17 時点の DOM に `.tabs-nav` 配下のボタン要素なし。`tab-panel` は dashboard のみ表示、他は hidden。

取得手順（手動/自動いずれか）:
1. `pnpm start` 等で UI を起動し、`http://localhost:5173` にアクセス。
2. DevTools で以下を確認:
   - `document.querySelectorAll('.tabs-nav button').length === 0` → OK
   - `document.querySelector('#tab-dashboard').classList.contains('tab-panel--active') === true`
   - `['agents','models','history','logs'].every(id => document.querySelector('#tab-'+id).hidden === true)`

備考:
- JS (`app.js`) の `initTabs` は no-op。タブ切り替えは存在しない。
- このメモは UI スナップショット代替の検証手順として保持。
