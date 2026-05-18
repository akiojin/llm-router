// API module re-exports
// All imports from '@/lib/api' continue to work via this barrel file

export { ApiError, fetchWithAuth, getCsrfToken, API_BASE } from './client'

export { authApi } from './auth'
export type { RegisterRequest, RegisterResponse } from './auth'

export { dashboardApi } from './dashboard'
export type {
  DashboardStats,
  DashboardOperations,
  DashboardCapacity,
  DashboardActionItem,
  SyncState,
  SyncProgress,
  RequestHistoryItem,
  RequestResponseRecord,
  RequestResponsesPage,
  EndpointTpsSummary,
  TpsApiKind,
  TpsSource,
  DashboardOverview,
  LogEntry,
  LogResponse,
  TokenStats,
  DailyTokenStats,
  MonthlyTokenStats,
} from './dashboard'

export {
  endpointsApi,
  benchmarkApi,
  CREATE_ENDPOINT_TIMEOUT_GUIDANCE,
  getRecommendedInferenceTimeout,
  getRecommendedInferenceTimeoutLabel,
} from './endpoints'
export type {
  EndpointType,
  DashboardEndpoint,
  DownloadTask,
  ModelMetadata,
  EndpointTodayStats,
  EndpointDailyStatEntry,
  ModelStatEntry,
  ModelTpsEntry,
  TpsBenchmarkRequest,
  TpsBenchmarkEndpointSummary,
  TpsBenchmarkResult,
  TpsBenchmarkRun,
  TpsBenchmarkAccepted,
} from './endpoints'

export { modelsApi } from './models'
export type {
  LifecycleStatus,
  DownloadProgress,
  ModelCapabilities,
  OpenAIModel,
  GgufFileInfo,
  GgufDiscoveryResult,
  DiscoverGgufResponse,
  OpenAIModelsResponse,
  RegisteredModelView,
} from './models'

export { chatApi } from './chat'
export type {
  ChatMessage,
  ChatSession,
  ChatCompletionRequest,
} from './chat'

export { systemApi } from './system'
export type {
  UpdatePayloadState,
  UpdateState,
  SystemInfo,
  ScheduleInfo,
  ApplyUpdateResponse,
  ForceApplyUpdateResponse,
  CreateScheduleRequest,
  RollbackResponse,
  VersionResponse,
} from './system'

export { settingsApi } from './settings'
export type { ApiKeyRequiredSetting, SettingSource } from './settings'

export { apiKeysApi } from './api-keys'
export type {
  ApiKeyPermission,
  ApiKey,
  CreateApiKeyResponse,
} from './api-keys'

export { invitationsApi } from './invitations'
export type {
  Invitation,
  CreateInvitationResponse,
} from './invitations'

export { usersApi } from './users'
export type { User, CreateUserResponse } from './users'

export { auditLogApi } from './audit-log'
export type {
  AuditLogEntry,
  AuditLogListResponse,
  AuditLogStatsResponse,
  HashChainVerifyResult,
  AuditLogFilters,
} from './audit-log'

export { catalogApi } from './catalog'
export type {
  CatalogSearchResult,
  CatalogSearchResponse,
  CatalogModelDetail,
  RecommendedEndpoint,
} from './catalog'

export { clientsApi } from './clients'
export type {
  ClientIpRanking,
  ClientRankingResponse,
  UniqueIpTimelinePoint,
  ModelDistribution,
  HeatmapCell,
  ClientDetailResponse,
  ClientRecentRequest,
  HourlyPattern,
  ClientApiKeyUsage,
} from './clients'
