// APIキー管理のJavaScript (T080-T082)

(function () {
  'use strict';

  const apiKeysTbody = document.getElementById('api-keys-tbody');
  const createApiKeyButton = document.getElementById('create-api-key-button');
  const apiKeyModal = document.getElementById('api-key-modal');
  const apiKeyModalClose = document.getElementById('api-key-modal-close');
  const apiKeyModalCancel = document.getElementById('api-key-modal-cancel');
  const apiKeyModalCreate = document.getElementById('api-key-modal-create');
  const apiKeyForm = document.getElementById('api-key-form');
  const apiKeyNameInput = document.getElementById('api-key-name');
  const apiKeyExpiresInput = document.getElementById('api-key-expires');

  const apiKeyDisplayModal = document.getElementById('api-key-display-modal');
  const apiKeyDisplayClose = document.getElementById('api-key-display-close');
  const apiKeyDisplayOk = document.getElementById('api-key-display-ok');
  const apiKeyDisplayCopy = document.getElementById('api-key-display-copy');
  const apiKeyDisplayValue = document.getElementById('api-key-display-value');

  let apiKeys = [];

  // APIキー一覧を読み込む（T080）
  async function loadApiKeys() {
    try {
      const response = await authenticatedFetch('/api/api-keys');
      if (response.ok) {
        apiKeys = await response.json();
        renderApiKeys();
      } else {
        showError('APIキーの読み込みに失敗しました');
      }
    } catch (error) {
      console.error('Failed to load API keys:', error);
      showError('APIキーの読み込みに失敗しました');
    }
  }

  // APIキー一覧を表示（T080）
  function renderApiKeys() {
    if (apiKeys.length === 0) {
      apiKeysTbody.innerHTML = '<tr><td colspan="5" class="empty-message">APIキーがありません</td></tr>';
      return;
    }

    apiKeysTbody.innerHTML = apiKeys
      .map((key) => {
        const createdAt = new Date(key.created_at).toLocaleString('ja-JP');
        const expiresAt = key.expires_at ? new Date(key.expires_at).toLocaleString('ja-JP') : '無期限';

        return `
          <tr>
            <td style="font-family: monospace; font-size: 0.85em;">${key.id.substring(0, 8)}...</td>
            <td>${escapeHtml(key.name)}</td>
            <td>${createdAt}</td>
            <td>${expiresAt}</td>
            <td>
              <button class="btn btn--danger btn--small delete-api-key" data-id="${key.id}">削除</button>
            </td>
          </tr>
        `;
      })
      .join('');

    // 削除ボタンのイベントリスナーを追加（T082）
    document.querySelectorAll('.delete-api-key').forEach((btn) => {
      btn.addEventListener('click', function () {
        const keyId = this.dataset.id;
        deleteApiKey(keyId);
      });
    });
  }

  // APIキーを発行（T081）
  async function createApiKey() {
    const name = apiKeyNameInput.value.trim();
    const expiresAtValue = apiKeyExpiresInput.value;

    if (!name) {
      alert('キーの名前を入力してください');
      return;
    }

    let expiresAt = null;
    if (expiresAtValue) {
      expiresAt = new Date(expiresAtValue).toISOString();
    }

    try {
      const response = await authenticatedFetch('/api/api-keys', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          name,
          expires_at: expiresAt,
        }),
      });

      if (response.ok) {
        const data = await response.json();
        closeApiKeyModal();
        showApiKeyDisplay(data.key);
        loadApiKeys();
      } else {
        const error = await response.json().catch(() => ({}));
        alert(error.error || 'APIキーの発行に失敗しました');
      }
    } catch (error) {
      console.error('Failed to create API key:', error);
      alert('APIキーの発行に失敗しました');
    }
  }

  // APIキーを削除（T082）
  async function deleteApiKey(keyId) {
    if (!confirm('このAPIキーを削除しますか？')) {
      return;
    }

    try {
      const response = await authenticatedFetch(`/api/api-keys/${keyId}`, {
        method: 'DELETE',
      });

      if (response.ok || response.status === 204) {
        loadApiKeys();
      } else {
        alert('APIキーの削除に失敗しました');
      }
    } catch (error) {
      console.error('Failed to delete API key:', error);
      alert('APIキーの削除に失敗しました');
    }
  }

  // APIキー発行モーダルを開く
  function openApiKeyModal() {
    apiKeyForm.reset();
    apiKeyModal.classList.remove('hidden');
  }

  // APIキー発行モーダルを閉じる
  function closeApiKeyModal() {
    apiKeyModal.classList.add('hidden');
    apiKeyForm.reset();
  }

  // APIキー表示モーダルを表示
  function showApiKeyDisplay(key) {
    apiKeyDisplayValue.value = key;
    apiKeyDisplayModal.classList.remove('hidden');
  }

  // APIキー表示モーダルを閉じる
  function closeApiKeyDisplayModal() {
    apiKeyDisplayModal.classList.add('hidden');
    apiKeyDisplayValue.value = '';
  }

  // APIキーをクリップボードにコピー
  function copyApiKeyToClipboard() {
    apiKeyDisplayValue.select();
    document.execCommand('copy');
    alert('APIキーをコピーしました');
  }

  // エラーメッセージを表示
  function showError(message) {
    apiKeysTbody.innerHTML = `<tr><td colspan="5" class="empty-message" style="color: #c53030;">${escapeHtml(message)}</td></tr>`;
  }

  // HTMLエスケープ
  function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  // イベントリスナー
  createApiKeyButton.addEventListener('click', openApiKeyModal);
  apiKeyModalClose.addEventListener('click', closeApiKeyModal);
  apiKeyModalCancel.addEventListener('click', closeApiKeyModal);
  apiKeyModalCreate.addEventListener('click', createApiKey);

  apiKeyDisplayClose.addEventListener('click', closeApiKeyDisplayModal);
  apiKeyDisplayOk.addEventListener('click', closeApiKeyDisplayModal);
  apiKeyDisplayCopy.addEventListener('click', copyApiKeyToClipboard);

  // タブが開かれたときにAPIキーを読み込む
  document.querySelectorAll('.tab-button').forEach((btn) => {
    btn.addEventListener('click', function () {
      if (this.dataset.tab === 'api-keys') {
        loadApiKeys();
      }
    });
  });

  // 初期読み込み（APIキータブがアクティブの場合）
  const currentTab = document.querySelector('.tab-button--active');
  if (currentTab && currentTab.dataset.tab === 'api-keys') {
    loadApiKeys();
  }
})();
