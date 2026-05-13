import { type Page, type Locator, expect } from '@playwright/test';
import { DashboardSelectors } from '../helpers/selectors';

/**
 * Page Object Model for LLM Router Dashboard
 */
export class DashboardPage {
  readonly page: Page;

  // Header Controls
  readonly themeToggle: Locator;
  readonly playgroundButton: Locator;
  readonly apiKeysButton: Locator;
  readonly auditLogButton: Locator;
  readonly refreshButton: Locator;
  readonly connectionStatus: Locator;
  readonly currentVersion: Locator;
  readonly userDropdownTrigger: Locator;

  // Stats
  readonly totalEndpoints: Locator;
  readonly totalRequests: Locator;
  readonly successRate: Locator;
  readonly outputTps: Locator;

  // Models - Tabs
  // NOTE: Model Hub タブは SPEC-6cd7f960 により廃止されました
  readonly localTab: Locator;
  readonly localModelsList: Locator;
  // Models - Local tab elements
  readonly localSearch: Locator;
  readonly registerButton: Locator;
  // Models - Registration dialog
  readonly registerModal: Locator;
  readonly registerRepo: Locator;
  readonly registerSubmit: Locator;

  // Nodes
  readonly nodesBody: Locator;
  readonly filterStatus: Locator;
  readonly filterQuery: Locator;
  readonly exportJson: Locator;
  readonly exportCsv: Locator;

  // Modals
  readonly chatModal: Locator;
  readonly chatClose: Locator;
  readonly apiKeysModal: Locator;
  readonly userModal: Locator;
  readonly invitationModal: Locator;

  constructor(page: Page) {
    this.page = page;

    // Header
    this.themeToggle = page.locator(DashboardSelectors.header.themeToggle);
    this.playgroundButton = page.locator(DashboardSelectors.header.playgroundButton);
    this.apiKeysButton = page.locator(DashboardSelectors.header.apiKeysButton);
    this.auditLogButton = page.locator(DashboardSelectors.header.auditLogButton);
    this.refreshButton = page.locator(DashboardSelectors.header.refreshButton);
    this.connectionStatus = page.locator(DashboardSelectors.header.connectionStatus);
    this.currentVersion = page.locator(DashboardSelectors.header.currentVersion);
    this.userDropdownTrigger = page.locator(DashboardSelectors.header.userDropdownTrigger);

    // Stats
    this.totalEndpoints = page.locator(DashboardSelectors.stats.totalEndpoints);
    this.totalRequests = page.locator(DashboardSelectors.stats.totalRequests);
    this.successRate = page.locator(DashboardSelectors.stats.successRate);
    this.outputTps = page.locator(DashboardSelectors.stats.outputTps);

    // Models - Tabs
    // NOTE: Model Hub タブは SPEC-6cd7f960 により廃止されました
    this.localTab = page.locator(DashboardSelectors.models.localTab);
    this.localModelsList = page.locator(DashboardSelectors.models.localModelsList);
    // Models - Local tab elements
    this.localSearch = page.locator(DashboardSelectors.models.localSearch);
    this.registerButton = page.locator(DashboardSelectors.models.registerButton);
    // Models - Registration dialog
    this.registerModal = page.locator(DashboardSelectors.models.registerModal);
    this.registerRepo = page.locator(DashboardSelectors.models.registerRepo);
    this.registerSubmit = page.locator(DashboardSelectors.models.registerSubmit);

    // Nodes
    this.nodesBody = page.locator(DashboardSelectors.nodes.nodesBody);
    this.filterStatus = page.locator(DashboardSelectors.nodes.filterStatus);
    this.filterQuery = page.locator(DashboardSelectors.nodes.filterQuery);
    this.exportJson = page.locator(DashboardSelectors.nodes.exportJson);
    this.exportCsv = page.locator(DashboardSelectors.nodes.exportCsv);

    // Modals
    this.chatModal = page.locator(DashboardSelectors.modals.chatModal);
    this.chatClose = page.locator(DashboardSelectors.modals.chatClose);
    this.apiKeysModal = page.locator(DashboardSelectors.modals.apiKeysModal);
    this.userModal = page.locator(DashboardSelectors.modals.userModal);
    this.invitationModal = page.locator(DashboardSelectors.modals.invitationModal);
  }

  async goto() {
    await this.page.goto('/dashboard');
    // Wait for page to settle (use 'load' instead of 'networkidle' due to WebSocket connections)
    await this.page.waitForLoadState('load');
    // Wait a moment for any JavaScript redirects
    await this.page.waitForTimeout(500);
    // Handle login if redirected to login page or login form appears after redirect
    const loginForm = this.page
      .locator('form')
      .filter({ hasText: 'Sign in' });
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const isLoginUrl = this.page.url().includes('login');
      const hasLoginForm = await loginForm
        .isVisible({ timeout: 2000 })
        .catch(() => false);
      if (isLoginUrl || hasLoginForm) {
        await this.login();
      }
      try {
        // Wait for dashboard content to be visible
        await this.page.waitForSelector('#theme-toggle', { timeout: 10000 });
        return;
      } catch (error) {
        if (attempt === 0) {
          continue;
        }
        throw error;
      }
    }
  }

  async gotoModels() {
    await this.goto();
    // Navigate to Models tab
    await this.page.click('button[role="tab"]:has-text("Models")');
    await this.page.waitForTimeout(500);
  }

  async gotoLocalModels() {
    await this.gotoModels();
    // Click Local tab
    await this.localTab.click();
    await this.page.waitForTimeout(300);
  }

  /**
   * @deprecated Model Hub タブは SPEC-6cd7f960 により廃止されました
   * Local タブで登録ダイアログを使用してください
   */
  async gotoModelHub() {
    // Model Hub タブは廃止されたため、Local タブに遷移
    await this.gotoLocalModels();
  }

  async login(username = 'admin', password = 'test') {
    await this.page.fill('#username', username);
    await this.page.fill('#password', password);
    await this.page.click('button[type="submit"]');
    // Wait for redirect to dashboard (the URL will NOT contain 'login' after successful login)
    await this.page.waitForFunction(() => !window.location.href.includes('login'), { timeout: 10000 });
    // Use 'load' instead of 'networkidle' due to WebSocket connections
    await this.page.waitForLoadState('load');
  }

  async toggleTheme() {
    await this.themeToggle.click();
  }

  /**
   * Opens the load balancer playground in the same tab.
   */
  async openPlayground() {
    await this.playgroundButton.click();
    await this.page.waitForURL('**/dashboard/#lb-playground');
    return this.page;
  }

  /**
   * @deprecated Playground is now a separate page, not a modal
   */
  async closePlayground() {
    // No-op - Playground is a separate page now
  }

  async openApiKeys() {
    await this.apiKeysButton.click();
    await expect(this.apiKeysModal).toBeVisible();
  }

  /**
   * Opens the audit log page (admin only).
   */
  async openAuditLog() {
    await this.auditLogButton.click();
    await this.page.waitForURL('**/dashboard/#audit-log');
  }

  /**
   * Opens the user dropdown menu.
   */
  async openUserDropdown() {
    await this.userDropdownTrigger.click();
    await this.page.waitForTimeout(200);
  }

  /**
   * Signs out via the user dropdown menu.
   */
  async signOut() {
    await this.openUserDropdown();
    await this.page.locator(DashboardSelectors.userDropdown.signOut).click();
    await this.page.waitForURL('**/login**');
  }

  /**
   * Opens Manage Users modal via the user dropdown (admin only).
   */
  async openManageUsersModal() {
    await this.openUserDropdown();
    await this.page.locator(DashboardSelectors.userDropdown.manageUsers).click();
    await expect(this.userModal).toBeVisible();
  }

  /**
   * Opens Invitation Codes modal via the user dropdown (admin only).
   */
  async openInvitationModal() {
    await this.openUserDropdown();
    await this.page.locator(DashboardSelectors.userDropdown.invitationCodes).click();
    await expect(this.invitationModal).toBeVisible();
  }

  /**
   * Navigates to a specific tab by name.
   */
  async goToTab(tabName: string) {
    await this.page.click(`button[role="tab"]:has-text("${tabName}")`);
    await this.page.waitForTimeout(300);
  }

  /**
   * Takes a screenshot and saves it to reports/screenshots/.
   */
  async takeScreenshot(name: string) {
    await this.page.screenshot({
      path: `reports/screenshots/${name}.png`,
      fullPage: true,
    });
  }

  async refresh() {
    await this.refreshButton.click();
  }

  async filterNodesByStatus(status: 'all' | 'online' | 'pending' | 'offline' | 'error') {
    await this.filterStatus.selectOption(status);
  }

  async searchNodes(query: string) {
    await this.filterQuery.fill(query);
  }

  /**
   * Search local models
   */
  async searchLocalModels(query: string) {
    await this.localSearch.fill(query);
    await this.page.waitForTimeout(300);
  }

  /**
   * @deprecated Use searchLocalModels instead
   */
  async searchHubModels(query: string) {
    await this.searchLocalModels(query);
  }

  /**
   * Open the registration dialog and register a model by repo
   */
  async registerModelByRepo(repo: string, filename?: string) {
    // Open register dialog
    await this.registerButton.click();
    await this.page.waitForTimeout(300);

    // Fill in the repo
    await this.registerRepo.fill(repo);

    // Fill in filename if provided
    if (filename) {
      const filenameInput = this.page.locator(DashboardSelectors.models.registerFilename);
      await filenameInput.fill(filename);
    }

    // Submit
    await this.registerSubmit.click();
    await this.page.waitForTimeout(500);
  }

  /**
   * @deprecated Model Hub は廃止されました。registerModelByRepo を使用してください
   */
  async registerModel(modelId: string) {
    // Model Hub は廃止されたため、repo として登録
    await this.registerModelByRepo(modelId);
  }

  /**
   * Get count of local models displayed
   */
  async getLocalModelCount(): Promise<number> {
    const cards = this.localModelsList.locator('.overflow-hidden');
    return cards.count();
  }

  /**
   * @deprecated Model Hub は廃止されました。getLocalModelCount を使用してください
   */
  async getHubModelCount(): Promise<number> {
    return this.getLocalModelCount();
  }

  async getConnectionStatus(): Promise<string> {
    return (await this.connectionStatus.textContent()) ?? '';
  }

  async getCurrentVersion(): Promise<string> {
    return (await this.currentVersion.textContent()) ?? '';
  }

  async getTotalEndpoints(): Promise<string> {
    return (await this.totalEndpoints.textContent()) ?? '-';
  }

  async getNodeCount(): Promise<number> {
    const rows = this.nodesBody.locator('tr:not(.empty-row)');
    return rows.count();
  }

  async sortBy(column: 'machine' | 'status' | 'uptime' | 'total') {
    const selector = `th[data-sort="${column}"]`;
    await this.page.click(selector);
  }

  // --- Statistics Tab (T008) ---

  async goToStatisticsTab() {
    await this.page.click('button[role="tab"]:has-text("Statistics")');
    await this.page.waitForTimeout(300);
  }

  async switchToDaily() {
    await this.page.click('button:has-text("Daily")');
    await this.page.waitForTimeout(300);
  }

  async switchToMonthly() {
    await this.page.click('button:has-text("Monthly")');
    await this.page.waitForTimeout(300);
  }

  async getTokenStatsCards(): Promise<{
    totalRequests: string;
    inputTokens: string;
    outputTokens: string;
    totalTokens: string;
  }> {
    const getText = async (selector: string) =>
      (await this.page.locator(selector).textContent()) ?? '';
    return {
      totalRequests: await getText('[data-stat="total-requests"]'),
      inputTokens: await getText('[data-stat="input-tokens"]'),
      outputTokens: await getText('[data-stat="output-tokens"]'),
      totalTokens: await getText('[data-stat="total-tokens"]'),
    };
  }

  // --- History Tab (T009) ---

  async goToHistoryTab() {
    await this.page.click('button[role="tab"]:has-text("History")');
    await this.page.waitForTimeout(300);
  }

  getHistoryRows(): Locator {
    return this.page.locator(DashboardSelectors.history.historyTbody).locator('tr');
  }

  async clickHistoryRow(index: number) {
    await this.getHistoryRows().nth(index).click();
    await this.page.waitForTimeout(300);
  }

  getHistoryDetailModal(): Locator {
    return this.page.locator(DashboardSelectors.modals.requestModal);
  }

  async switchDetailTab(tab: 'overview' | 'request' | 'response') {
    const modal = this.getHistoryDetailModal();
    await modal.locator(`button[role="tab"]:has-text("${tab}")`).click({ force: true });
    await this.page.waitForTimeout(300);
  }

  async goToHistoryPage(direction: 'next' | 'prev') {
    const selector =
      direction === 'next'
        ? DashboardSelectors.history.pageNext
        : DashboardSelectors.history.pagePrev;
    await this.page.click(selector);
    await this.page.waitForTimeout(300);
  }

  getHistoryPagination(): Locator {
    return this.page.locator(DashboardSelectors.history.pageInfo);
  }

  // --- Logs Tab (T010) ---

  async goToLogsTab() {
    await this.page.click('button[role="tab"]:has-text("Logs")');
    await this.page.waitForTimeout(300);
  }

  getRouterLogEntries(): Locator {
    return this.page.locator(DashboardSelectors.logs.routerList).locator('> *');
  }

  async clickRefreshRouterLogs() {
    await this.page.click(DashboardSelectors.logs.routerRefresh);
    await this.page.waitForTimeout(300);
  }

  async selectEndpointForLogs(endpointName: string) {
    await this.page.locator(DashboardSelectors.logs.nodeSelect).selectOption({ label: endpointName });
    await this.page.waitForTimeout(300);
  }

  getEndpointLogEntries(): Locator {
    return this.page.locator(DashboardSelectors.logs.nodeList).locator('> *');
  }

  async clickRefreshEndpointLogs() {
    await this.page.click(DashboardSelectors.logs.nodeRefresh);
    await this.page.waitForTimeout(300);
  }

  // --- User Management (T011) ---

  async openUserManagement() {
    // Click user dropdown in header, then click "Users" menu item
    await this.page.click('#user-menu-button');
    await this.page.waitForTimeout(200);
    await this.page.click('a:has-text("Users"), button:has-text("Users")');
    await this.page.waitForTimeout(300);
  }

  getUserRows(): Locator {
    return this.page.locator('#users-table tbody tr, #user-list .user-row');
  }

  async clickAddUser() {
    await this.page.click('button:has-text("Add User")');
    await this.page.waitForTimeout(300);
  }

  async fillUserForm(username: string, password: string, role: string) {
    await this.page.fill('#new-username, input[name="username"]', username);
    await this.page.fill('#new-password, input[name="password"]', password);
    await this.page.locator('#new-role, select[name="role"]').selectOption(role);
  }

  async submitUserForm() {
    await this.page.click('button[type="submit"]:has-text("Create"), button:has-text("Save")');
    await this.page.waitForTimeout(300);
  }

  // --- Endpoint Edit (T012) ---

  async openEndpointDetail(endpointName: string) {
    const row = this.nodesBody.locator('tr', { hasText: endpointName });
    await row.locator('button[title="Details"]').click();
    await this.page.waitForTimeout(300);
  }

  async editDisplayName(newName: string) {
    const modal = this.page.locator(DashboardSelectors.modals.nodeModal);
    await modal.locator('input[name="display_name"], #node-display-name').fill(newName);
  }

  async editHealthCheckInterval(seconds: number) {
    const modal = this.page.locator(DashboardSelectors.modals.nodeModal);
    await modal
      .locator('input[name="health_check_interval"], #node-health-check-interval')
      .fill(String(seconds));
  }

  async editInferenceTimeout(seconds: number) {
    const modal = this.page.locator(DashboardSelectors.modals.nodeModal);
    await modal
      .locator('input[name="inference_timeout"], #node-inference-timeout')
      .fill(String(seconds));
  }

  async editNotes(text: string) {
    const modal = this.page.locator(DashboardSelectors.modals.nodeModal);
    await modal.locator('textarea[name="notes"], #node-notes').fill(text);
  }

  async saveEndpointChanges() {
    await this.page.click(DashboardSelectors.modals.nodeModalSave);
    await this.page.waitForTimeout(300);
  }
}
