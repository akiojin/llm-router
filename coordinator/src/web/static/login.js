// ログイン画面のJavaScript

(function () {
  'use strict';

  const loginForm = document.getElementById('login-form');
  const loginButton = document.getElementById('login-button');
  const errorMessage = document.getElementById('error-message');
  const usernameInput = document.getElementById('username');
  const passwordInput = document.getElementById('password');

  // エラーメッセージを表示
  function showError(message) {
    errorMessage.textContent = message;
    errorMessage.classList.add('visible');
  }

  // エラーメッセージを非表示
  function hideError() {
    errorMessage.classList.remove('visible');
  }

  // ログインボタンを無効化
  function disableLoginButton() {
    loginButton.disabled = true;
    loginButton.textContent = 'ログイン中...';
  }

  // ログインボタンを有効化
  function enableLoginButton() {
    loginButton.disabled = false;
    loginButton.textContent = 'ログイン';
  }

  // ログイン処理
  async function handleLogin(event) {
    event.preventDefault();
    hideError();

    const username = usernameInput.value.trim();
    const password = passwordInput.value;

    if (!username || !password) {
      showError('ユーザー名とパスワードを入力してください');
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
        // JWTトークンをlocalStorageに保存
        localStorage.setItem('jwt_token', data.token);
        // ダッシュボードにリダイレクト
        window.location.href = '/dashboard';
      } else if (response.status === 401) {
        showError('ユーザー名またはパスワードが正しくありません');
        enableLoginButton();
      } else {
        const errorData = await response.json().catch(() => ({}));
        showError(errorData.error || 'ログインに失敗しました');
        enableLoginButton();
      }
    } catch (error) {
      console.error('Login error:', error);
      showError('ネットワークエラーが発生しました');
      enableLoginButton();
    }
  }

  // フォーム送信イベントリスナー
  loginForm.addEventListener('submit', handleLogin);

  // 既にログイン済みの場合はダッシュボードにリダイレクト
  const token = localStorage.getItem('jwt_token');
  if (token) {
    // トークンの有効性を確認
    fetch('/api/auth/me', {
      headers: {
        Authorization: `Bearer ${token}`,
      },
    })
      .then((response) => {
        if (response.ok) {
          window.location.href = '/dashboard';
        } else {
          // トークンが無効なら削除
          localStorage.removeItem('jwt_token');
        }
      })
      .catch(() => {
        // ネットワークエラーの場合は無視（ログイン画面を表示）
      });
  }
})();
