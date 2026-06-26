import { test, expect, APIRequestContext } from '@playwright/test';
import { RegisterPage, LoginPage } from '../../pages/auth.page';

const API_BASE = process.env.BASE_URL || 'http://127.0.0.1:32768';

/**
 * Helper to get JWT token for admin user
 */
async function getAdminToken(request: APIRequestContext): Promise<string> {
  const response = await request.post(`${API_BASE}/api/auth/login`, {
    data: {
      username: 'admin',
      password: 'test',
    },
  });

  if (response.status() !== 200) {
    throw new Error(`Login failed: ${response.status()}`);
  }

  const data = await response.json();
  return data.token;
}

/**
 * Helper to create invitation code via API
 */
async function createInvitationCode(request: APIRequestContext): Promise<string> {
  const token = await getAdminToken(request);

  const response = await request.post(`${API_BASE}/api/invitations`, {
    headers: {
      Authorization: `Bearer ${token}`,
    },
    data: {
      expires_in_hours: 24,
    },
  });

  if (response.status() !== 201) {
    throw new Error(`Failed to create invitation: ${response.status()}`);
  }

  const data = await response.json();
  return data.code;
}

test.describe('Register Page', () => {
  let registerPage: RegisterPage;

  test.beforeEach(async ({ page }) => {
    registerPage = new RegisterPage(page);
    await registerPage.goto();
    // Wait for page to fully load
    await page.waitForLoadState('networkidle');
  });

  test('R-01: Register page is accessible', async ({ page }) => {
    await expect(page).toHaveTitle(/Register.*LLM Load Balancer/);
  });

  test('R-02: Register form elements are visible', async () => {
    await expect(registerPage.invitationCodeInput).toBeVisible();
    await expect(registerPage.usernameInput).toBeVisible();
    await expect(registerPage.passwordInput).toBeVisible();
    await expect(registerPage.confirmPasswordInput).toBeVisible();
    await expect(registerPage.submitButton).toBeVisible();
  });

  test('R-03: Login link is visible', async ({ page }) => {
    const loginLink = page.locator('a:has-text("Sign in")');
    await expect(loginLink).toBeVisible();
  });

  test('R-04: Invalid invitation code shows error', async ({ page }) => {
    await registerPage.register('inv_invalidcode123456', 'testuser', 'Password123');

    // Wait for error toast (Toaster component)
    const toast = page.locator('[data-sonner-toast], [role="status"], .toast');
    await expect(toast.first()).toBeVisible({ timeout: 5000 });
  });

  test('R-05: Successful registration with valid code', async ({ page, request }) => {
    // Create a valid invitation code via API
    const invitationCode = await createInvitationCode(request);

    // Generate unique username
    const username = `testuser_${Date.now()}_${Math.random().toString(16).slice(2)}`;

    // Register with the code
    await registerPage.register(invitationCode, username, 'Password123');

    // Wait for success message
    await registerPage.waitForSuccess();

    // Click "Go to Sign In" button
    const signInButton = page.locator('button:has-text("Go to Sign In")');
    await signInButton.click();

    // Should be on login page
    await page.waitForURL(/login/);

    // Log in with new credentials
    const loginPage = new LoginPage(page);
    await loginPage.login(username, 'Password123');
    await loginPage.waitForDashboard();

    // Should be logged in successfully
    expect(page.url()).not.toContain('login');
  });

  test('R-06: Used invitation code is rejected', async ({ page, request }) => {
    // Create and use an invitation code
    const invitationCode = await createInvitationCode(request);
    const username1 = `testuser1_${Date.now()}_${Math.random().toString(16).slice(2)}`;

    // First registration
    await registerPage.register(invitationCode, username1, 'Password123');
    await registerPage.waitForSuccess();

    // Navigate back to register page
    await registerPage.goto();

    // Try to use the same code again
    const username2 = `testuser2_${Date.now()}_${Math.random().toString(16).slice(2)}`;
    await registerPage.register(invitationCode, username2, 'Password123');

    // Should show error
    await expect(registerPage.errorToast).toBeVisible({ timeout: 5000 });
  });

  test('R-07: Password mismatch shows error', async ({ page }) => {
    await registerPage.invitationCodeInput.fill('inv_anycode12345678');
    await registerPage.usernameInput.fill('testuser');
    await registerPage.passwordInput.fill('Password123');
    await registerPage.confirmPasswordInput.fill('differentpassword');
    await registerPage.submitButton.click();

    // Should show error toast about password mismatch
    const toast = page.locator('[data-sonner-toast], [role="status"], .toast');
    await expect(toast.first()).toBeVisible({ timeout: 5000 });
  });

  test('R-08: Login link navigates to login page', async ({ page }) => {
    const loginLink = page.locator('a:has-text("Sign in")');
    await loginLink.click();
    await page.waitForURL(/login/, { timeout: 10000 });
    expect(page.url()).toContain('login');
  });
});
