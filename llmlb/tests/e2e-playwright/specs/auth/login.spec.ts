import { test, expect } from '@playwright/test';
import { LoginPage } from '../../pages/auth.page';

test.describe('Login Page', () => {
  let loginPage: LoginPage;

  test.beforeEach(async ({ page }) => {
    loginPage = new LoginPage(page);
    await loginPage.goto();
    // Wait for page to fully load
    await page.waitForLoadState('networkidle');
  });

  test('L-01: Login page is accessible', async ({ page }) => {
    await expect(page).toHaveTitle(/Login.*LLM Load Balancer/);
  });

  test('L-02: Login form elements are visible', async () => {
    await expect(loginPage.usernameInput).toBeVisible();
    await expect(loginPage.passwordInput).toBeVisible();
    await expect(loginPage.submitButton).toBeVisible();
  });

  test('L-03: Register link is visible', async ({ page }) => {
    // The register link contains "Register" text
    const registerLink = page.locator('a:has-text("Register")');
    await expect(registerLink).toBeVisible();
  });

  test('L-04: Successful login redirects to dashboard', async ({ page }) => {
    await loginPage.login('admin', 'test');
    await loginPage.waitForDashboard();

    // Should be on dashboard, not login page
    expect(page.url()).not.toContain('login');
    expect(page.url()).toContain('/dashboard/');
  });

  test('L-05: Invalid credentials show error toast', async ({ page }) => {
    await loginPage.login('invalid', 'wrongpassword');

    // Wait for error toast (Toaster component)
    const toast = page.locator('[data-sonner-toast], [role="status"], .toast');
    await expect(toast.first()).toBeVisible({ timeout: 5000 });
  });

  test('L-06: Empty username is rejected', async ({ page }) => {
    await loginPage.passwordInput.fill('somepassword');
    await loginPage.submitButton.click();

    // Form validation should prevent submission
    // Username input should have validation error
    const usernameInput = loginPage.usernameInput;
    await expect(usernameInput).toHaveAttribute('required', '');
  });

  test('L-07: Register link navigates to register page', async ({ page }) => {
    const registerLink = page.locator('a:has-text("Register")');
    await registerLink.click();
    await page.waitForURL(/register/, { timeout: 10000 });
    expect(page.url()).toContain('register');
  });

  test('L-08: Language switcher toggles English/Japanese (i18n)', async ({ page }) => {
    // Default (locale en-US) renders English.
    await expect(page.getByText('Mission Control Dashboard')).toBeVisible();

    // Switch to Japanese.
    await page.getByRole('button', { name: 'Switch to Japanese' }).click();
    await expect(page.getByText('ミッションコントロール ダッシュボード')).toBeVisible({ timeout: 5000 });

    // Switch back to English.
    await page.getByRole('button', { name: 'Switch to English' }).click();
    await expect(page.getByText('Mission Control Dashboard')).toBeVisible({ timeout: 5000 });
  });
});
