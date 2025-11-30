// Login page JavaScript

(function () {
  'use strict';

  const loginForm = document.getElementById('login-form');
  const loginButton = document.getElementById('login-button');
  const errorMessage = document.getElementById('error-message');
  const usernameInput = document.getElementById('username');
  const passwordInput = document.getElementById('password');

  // Show error message
  function showError(message) {
    errorMessage.textContent = message;
    errorMessage.classList.add('visible');
  }

  // Hide error message
  function hideError() {
    errorMessage.classList.remove('visible');
  }

  // Disable login button
  function disableLoginButton() {
    loginButton.disabled = true;
    loginButton.textContent = 'Signing in...';
  }

  // Enable login button
  function enableLoginButton() {
    loginButton.disabled = false;
    loginButton.textContent = 'Sign In';
  }

  // Handle login
  async function handleLogin(event) {
    event.preventDefault();
    hideError();

    const username = usernameInput.value.trim();
    const password = passwordInput.value;

    if (!username || !password) {
      showError('Please enter username and password');
      return;
    }

    disableLoginButton();

    try {
      const response = await fetch('/api/auth/login', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          username,
          password,
        }),
      });

      if (response.ok) {
        const data = await response.json();
        // Save JWT token to localStorage
        localStorage.setItem('jwt_token', data.token);
        // Redirect to dashboard
        window.location.href = '/dashboard';
      } else if (response.status === 401) {
        showError('Invalid username or password');
        enableLoginButton();
      } else {
        const errorData = await response.json().catch(() => ({}));
        showError(errorData.error || 'Login failed');
        enableLoginButton();
      }
    } catch (error) {
      console.error('Login error:', error);
      showError('Network error occurred');
      enableLoginButton();
    }
  }

  // Form submit event listener
  loginForm.addEventListener('submit', handleLogin);

  // If already logged in, redirect to dashboard
  const token = localStorage.getItem('jwt_token');
  if (token) {
    // Verify token validity
    fetch('/api/auth/me', {
      headers: {
        Authorization: `Bearer ${token}`,
      },
    })
      .then((response) => {
        if (response.ok) {
          window.location.href = '/dashboard';
        } else {
          // Remove invalid token
          localStorage.removeItem('jwt_token');
        }
      })
      .catch(() => {
        // Ignore network errors (show login page)
      });
  }
})();
