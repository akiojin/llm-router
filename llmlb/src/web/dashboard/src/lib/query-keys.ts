/**
 * react-query の queryKey 定数/ファクトリの単一の真実源。
 *
 * arch-review [L21]: queryKey が各コンポーネントに文字列リテラルで散在し
 * （'dashboard-endpoints' だけで 11 箇所）、WebSocket 由来の invalidateQueries が
 * 「文字列が偶然一致すること」に依存していた。ここへ集約し、クエリ定義と無効化が
 * 同じキーを参照するようにすることでタイプミスによる無効化漏れを防ぐ。
 *
 * 各値は `as const` タプルで、react-query のキー比較（構造的等価）と互換。
 */
export const queryKeys = {
  // --- ダッシュボード全体 ---
  dashboardEndpoints: ['dashboard-endpoints'] as const,
  dashboardOverview: ['dashboard-overview'] as const,
  systemInfo: ['system-info'] as const,
  version: ['version'] as const,
  requestResponses: ['request-responses'] as const,
  routerLogs: ['router-logs'] as const,
  auditLogs: (filters: unknown) => ['audit-logs', filters] as const,

  // --- モデル ---
  models: ['models'] as const,
  viewerModels: (view: unknown) => ['viewer-models', view] as const,
  lbPlaygroundModels: ['lb-playground-models'] as const,
  allModelStats: ['all-model-stats'] as const,

  // --- カタログ ---
  catalogSearch: (query: unknown) => ['catalog-search', query] as const,
  catalogModel: (repoId: unknown) => ['catalog-model', repoId] as const,
  catalogRecommend: (repoId: unknown) => ['catalog-recommend', repoId] as const,

  // --- エンドポイント ---
  endpoint: (endpointId: unknown) => ['endpoint', endpointId] as const,
  endpointModels: (endpointId: unknown) => ['endpoint-models', endpointId] as const,
  endpointModelStats: (endpointId: unknown) => ['endpoint-model-stats', endpointId] as const,
  endpointModelTps: (endpointId: unknown) => ['endpoint-model-tps', endpointId] as const,
  endpointDailyStats: (endpointId: unknown, days: unknown) =>
    ['endpoint-daily-stats', endpointId, days] as const,
  endpointTodayStats: (endpointId: unknown) => ['endpoint-today-stats', endpointId] as const,

  // --- クライアント分析 ---
  clientRanking: (page: unknown, perPage: unknown, ipFilter: unknown) =>
    ['client-ranking', page, perPage, ipFilter] as const,
  clientHeatmap: (ipFilter: unknown) => ['client-heatmap', ipFilter] as const,
  clientDetail: (ip: unknown) => ['client-detail', ip] as const,
  clientTimeline: ['client-timeline'] as const,
  clientModels: ['client-models'] as const,
  clientApiKeys: (ip: unknown) => ['client-api-keys', ip] as const,

  // --- トークン統計 ---
  tokenStatsDaily: ['token-stats-daily'] as const,
  tokenStatsMonthly: ['token-stats-monthly'] as const,

  // --- 管理 ---
  users: ['users'] as const,
  apiKeys: ['api-keys'] as const,
  invitations: ['invitations'] as const,
  alertThreshold: ['alert-threshold'] as const,
  clientRankingPrefix: ['client-ranking'] as const,
} as const;
