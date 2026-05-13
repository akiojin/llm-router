/**
 * CSS Selectors for LLM Router Dashboard and Playground
 * Centralized selector definitions for maintainability
 */

export const DashboardSelectors = {
  // Header Controls
  header: {
    themeToggle: '#theme-toggle',
    playgroundButton: '#lb-playground-button',
    apiKeysButton: '#api-keys-button',
    auditLogButton: '#audit-log-button',
    refreshButton: '#refresh-button',
    connectionStatus: '#connection-status',
    currentVersion: '#current-version',
    lastRefreshed: '#last-refreshed',
    refreshMetrics: '#refresh-metrics',
    userDropdownTrigger: 'button:has(svg.lucide-user)',
  },

  // User Dropdown Menu Items
  userDropdown: {
    apiKeys: '[role="menuitem"]:has-text("API Keys")',
    lbPlayground: '[role="menuitem"]:has-text("LB Playground")',
    manageUsers: '[role="menuitem"]:has-text("Manage Users")',
    invitationCodes: '[role="menuitem"]:has-text("Invitation Codes")',
    auditLog: '[role="menuitem"]:has-text("Audit Log")',
    signOut: '[role="menuitem"]:has-text("Sign out")',
  },

  // Operations summary - these match the actual data-stat attributes in OperationsOverview.tsx
  stats: {
    operationalHealth: '[data-stat="operational-health"]',
    totalEndpoints: '[data-stat="total-endpoints"]',
    totalRequests: '[data-stat="total-requests"]',
    activeRequests: '[data-stat="active-requests"]',
    queuedRequests: '[data-stat="queued-requests"]',
    successRate: '[data-stat="success-rate"]',
    outputTps: '[data-stat="output-tps"]',
    totalTokens: '[data-stat="total-tokens"]',
    gpuCapacity: '[data-stat="gpu-capacity"]',
    actionItems: '[data-section="action-items"]',
  },

  // Models Tab
  // NOTE: Model Hub タブは SPEC-6cd7f960 により廃止されました
  models: {
    // Tab navigation
    localTab: 'button[role="tab"]:has-text("Local")',
    // hubTab は廃止 - Model Hub タブは削除されました
    // Model lists
    localModelsList: '#local-models-list',
    // Local tab elements
    localSearch: 'input[placeholder*="Search"]',
    registerButton: '#register-model',
    // Registration dialog elements
    registerModal: '#register-modal',
    registerRepo: '#register-repo',
    registerFilename: '#register-filename',
    registerDisplayName: '#register-display-name',
    registerSubmit: '#register-submit',
    registerCancel: '#register-modal-close',
    // Individual model card elements
    modelCard: '.model-card',
    modelName: '[data-model-name]',
    modelDescription: '[data-model-description]',
    modelSize: '[data-model-size]',
  },

  // Nodes Tab
  nodes: {
    nodesBody: '#nodes-body',
    filterStatus: '#filter-status',
    filterQuery: '#filter-query',
    selectAll: '#select-all',
    exportJson: '#export-json',
    exportCsv: '#export-csv',
    pagePrev: '#page-prev',
    pageNext: '#page-next',
    pageInfo: '#page-info',
    sortMachine: 'th[data-sort="machine"]',
    sortStatus: 'th[data-sort="status"]',
    sortUptime: 'th[data-sort="uptime"]',
    sortTotal: 'th[data-sort="total"]',
  },

  // Request History Tab
  history: {
    historyTbody: '#request-history-tbody',
    filterModel: '#filter-history-model',
    perPage: '#history-per-page',
    pagePrev: '#history-page-prev',
    pageNext: '#history-page-next',
    pageInfo: '#history-page-info',
    exportCsv: '#export-history-csv',
  },

  // Logs Tab
  logs: {
    routerList: '#logs-router-list',
    routerRefresh: '#logs-router-refresh',
    nodeSelect: '#logs-node-select',
    nodeList: '#logs-node-list',
    nodeRefresh: '#logs-node-refresh',
  },

  // Modals
  modals: {
    nodeModal: '#node-modal',
    nodeModalClose: '#node-modal-close',
    nodeModalSave: '#node-modal-save',
    nodeModalDisconnect: '#node-modal-disconnect',
    nodeModalDelete: '#node-modal-delete',
    requestModal: '#request-modal',
    requestModalClose: '#request-modal-close',
    apiKeysModal: '#api-keys-modal',
    apiKeysModalClose: '.modal__close',
    apiKeyName: '#api-key-name',
    createApiKey: '#create-api-key',
    copyApiKey: '#copy-api-key',
    userModal: '[role="dialog"]:has-text("Users")',
    invitationModal: '[role="dialog"]:has-text("Invitation")',
    chatModal: '#chat-modal',
    chatClose: '#chat-close',
  },

  // Error Banner
  errorBanner: '#error-banner',
  errorBannerClose: '#error-banner-close',
};

export const PlaygroundSelectors = {
  // Sidebar
  sidebar: {
    container: '#sidebar',
    toggle: '#sidebar-toggle',
    toggleMobile: '#sidebar-toggle-mobile',
    newChat: '#new-chat',
    sessionList: '#session-list',
  },

  // Header
  header: {
    modelSelect: '#model-select',
    routerStatus: '#router-status',
    settingsToggle: '#settings-toggle',
  },

  // Chat
  chat: {
    container: '.chat-container',
    messages: '#chat-log',
    input: '#chat-input',
    sendButton: '#send-button',
    stopButton: '#stop-button',
    form: '#chat-form',
    welcome: '.chat-welcome',
  },

  // Messages
  messages: {
    user: '.message--user',
    assistant: '.message--assistant',
    text: '.message-text',
  },

  // Error Banner
  errorBanner: '#error-banner',
  errorMessage: '#error-message',
  errorClose: '#error-close',

  // Settings Modal
  settings: {
    modal: '#settings-modal',
    close: '#modal-close',
    providerLocal: '.provider-btn[data-provider="local"]',
    providerCloud: '.provider-btn[data-provider="cloud"]',
    providerAll: '.provider-btn[data-provider="all"]',
    apiKeyInput: '#api-key-input',
    streamToggle: '#stream-toggle',
    appendSystem: '#append-system',
    systemPrompt: '#system-prompt',
    resetChat: '#reset-chat',
    copyCurl: '#copy-curl',
  },
};
