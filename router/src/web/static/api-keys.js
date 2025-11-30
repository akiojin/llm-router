// API Key Management JavaScript

(function () {
  'use strict';

  // DOM elements
  const apiKeysButton = document.getElementById('api-keys-button');
  const apiKeysModal = document.getElementById('api-keys-modal');
  const apiKeysModalClose = document.getElementById('api-keys-modal-close');
  const apiKeysModalOk = document.getElementById('api-keys-modal-ok');
  const apiKeysTbody = document.getElementById('api-keys-tbody');
  const apiKeyNameInput = document.getElementById('api-key-name');
  const apiKeyExpirySelect = document.getElementById('api-key-expiry');
  const createApiKeyButton = document.getElementById('create-api-key');
  const newKeyDisplay = document.getElementById('new-key-display');
  const newKeyValue = document.getElementById('new-key-value');
  const copyApiKeyButton = document.getElementById('copy-api-key');

  let apiKeys = [];
  let editingKeyId = null;

  // Open modal
  function openModal() {
    apiKeysModal.classList.remove('hidden');
    newKeyDisplay.classList.add('hidden');
    cancelEdit();
    loadApiKeys();
  }

  // Close modal
  function closeModal() {
    apiKeysModal.classList.add('hidden');
    newKeyDisplay.classList.add('hidden');
    cancelEdit();
  }

  // Cancel edit mode
  function cancelEdit() {
    editingKeyId = null;
    apiKeyNameInput.value = '';
    apiKeyExpirySelect.value = '';
    createApiKeyButton.textContent = 'Create';
    createApiKeyButton.classList.remove('btn--secondary');
    createApiKeyButton.classList.add('btn--primary');
  }

  // Load API keys list
  async function loadApiKeys() {
    try {
      const response = await authenticatedFetch('/api/api-keys');
      if (response.ok) {
        const data = await response.json();
        apiKeys = data.api_keys || data || [];
        renderApiKeys();
      } else if (response.status === 401 || response.status === 403) {
        showError('Authentication required. Please log in.');
      } else {
        showError('Failed to load API keys');
      }
    } catch (error) {
      console.error('Failed to load API keys:', error);
      showError('Failed to load API keys');
    }
  }

  // Render API keys list
  function renderApiKeys() {
    if (!Array.isArray(apiKeys) || apiKeys.length === 0) {
      apiKeysTbody.innerHTML = '<tr><td colspan="4" class="empty-message">No API keys</td></tr>';
      return;
    }

    apiKeysTbody.innerHTML = apiKeys
      .map((key) => {
        const createdAt = new Date(key.created_at).toLocaleString();
        const isUnlimited = !key.expires_at;
        const expiresAt = isUnlimited ? 'No expiry' : new Date(key.expires_at).toLocaleString();
        const warningMark = isUnlimited ? '<span class="unlimited-warning" title="No expiry API key">&#9888;</span> ' : '';

        return `
          <tr>
            <td>${escapeHtml(key.name)}</td>
            <td>${createdAt}</td>
            <td>${warningMark}${expiresAt}</td>
            <td class="api-key-actions">
              <button class="btn btn--small edit-api-key" data-id="${key.id}" data-name="${escapeHtml(key.name)}" data-expires="${key.expires_at || ''}">Edit</button>
              <button class="btn btn--danger btn--small delete-api-key" data-id="${key.id}">Delete</button>
            </td>
          </tr>
        `;
      })
      .join('');

    // Add event listeners for edit buttons
    document.querySelectorAll('.edit-api-key').forEach((btn) => {
      btn.addEventListener('click', function () {
        startEdit(this.dataset.id, this.dataset.name, this.dataset.expires);
      });
    });

    // Add event listeners for delete buttons
    document.querySelectorAll('.delete-api-key').forEach((btn) => {
      btn.addEventListener('click', function () {
        const keyId = this.dataset.id;
        deleteApiKey(keyId);
      });
    });
  }

  // Start edit mode
  function startEdit(keyId, name, expiresAt) {
    editingKeyId = keyId;
    apiKeyNameInput.value = name;

    // Calculate days from expiry (approximate)
    if (expiresAt) {
      const now = new Date();
      const expires = new Date(expiresAt);
      const diffDays = Math.round((expires - now) / (1000 * 60 * 60 * 24));

      // Select closest option
      if (diffDays <= 3) {
        apiKeyExpirySelect.value = '3';
      } else if (diffDays <= 7) {
        apiKeyExpirySelect.value = '7';
      } else if (diffDays <= 21) {
        apiKeyExpirySelect.value = '21';
      } else if (diffDays <= 70) {
        apiKeyExpirySelect.value = '70';
      } else {
        apiKeyExpirySelect.value = '';
      }
    } else {
      apiKeyExpirySelect.value = '';
    }

    createApiKeyButton.textContent = 'Update';
    createApiKeyButton.classList.remove('btn--primary');
    createApiKeyButton.classList.add('btn--secondary');
    apiKeyNameInput.focus();
  }

  // Create or update API key
  async function createOrUpdateApiKey() {
    const name = apiKeyNameInput.value.trim();
    const expiryDays = apiKeyExpirySelect.value;

    if (!name) {
      alert('Please enter a key name');
      return;
    }

    let expiresAt = null;
    if (expiryDays) {
      const date = new Date();
      date.setDate(date.getDate() + parseInt(expiryDays, 10));
      expiresAt = date.toISOString();
    }

    if (editingKeyId) {
      // Update mode
      await updateApiKey(editingKeyId, name, expiresAt);
    } else {
      // Create mode
      await createApiKey(name, expiresAt);
    }
  }

  // Create API key
  async function createApiKey(name, expiresAt) {
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
        showNewKey(data.key);
        cancelEdit();
        loadApiKeys();
      } else {
        const error = await response.json().catch(() => ({}));
        alert(error.message || error.error || 'Failed to create API key');
      }
    } catch (error) {
      console.error('Failed to create API key:', error);
      alert('Failed to create API key');
    }
  }

  // Update API key
  async function updateApiKey(keyId, name, expiresAt) {
    try {
      const response = await authenticatedFetch(`/api/api-keys/${keyId}`, {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          name,
          expires_at: expiresAt,
        }),
      });

      if (response.ok) {
        cancelEdit();
        loadApiKeys();
      } else if (response.status === 404) {
        alert('API key not found');
        cancelEdit();
        loadApiKeys();
      } else {
        const error = await response.json().catch(() => ({}));
        alert(error.message || error.error || 'Failed to update API key');
      }
    } catch (error) {
      console.error('Failed to update API key:', error);
      alert('Failed to update API key');
    }
  }

  // Show newly created key
  function showNewKey(key) {
    newKeyValue.textContent = key;
    newKeyDisplay.classList.remove('hidden');
  }

  // Delete API key
  async function deleteApiKey(keyId) {
    if (!confirm('Are you sure you want to delete this API key?')) {
      return;
    }

    try {
      const response = await authenticatedFetch(`/api/api-keys/${keyId}`, {
        method: 'DELETE',
      });

      if (response.ok || response.status === 204) {
        if (editingKeyId === keyId) {
          cancelEdit();
        }
        loadApiKeys();
      } else {
        alert('Failed to delete API key');
      }
    } catch (error) {
      console.error('Failed to delete API key:', error);
      alert('Failed to delete API key');
    }
  }

  // Copy to clipboard
  async function copyToClipboard() {
    const key = newKeyValue.textContent;
    try {
      await navigator.clipboard.writeText(key);
      alert('API key copied to clipboard');
    } catch (error) {
      // Fallback
      const textarea = document.createElement('textarea');
      textarea.value = key;
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand('copy');
      document.body.removeChild(textarea);
      alert('API key copied to clipboard');
    }
  }

  // Show error message
  function showError(message) {
    apiKeysTbody.innerHTML = `<tr><td colspan="4" class="empty-message" style="color: #c53030;">${escapeHtml(message)}</td></tr>`;
  }

  // HTML escape
  function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  // Event listeners
  if (apiKeysButton) {
    apiKeysButton.addEventListener('click', openModal);
  }
  if (apiKeysModalClose) {
    apiKeysModalClose.addEventListener('click', closeModal);
  }
  if (apiKeysModalOk) {
    apiKeysModalOk.addEventListener('click', closeModal);
  }
  if (createApiKeyButton) {
    createApiKeyButton.addEventListener('click', createOrUpdateApiKey);
  }
  if (copyApiKeyButton) {
    copyApiKeyButton.addEventListener('click', copyToClipboard);
  }

  // Close modal on background click
  if (apiKeysModal) {
    apiKeysModal.addEventListener('click', function (e) {
      if (e.target === apiKeysModal) {
        closeModal();
      }
    });
  }

  // Close on ESC key
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape' && apiKeysModal && !apiKeysModal.classList.contains('hidden')) {
      closeModal();
    }
  });
})();
