// ユーザー管理のJavaScript (T084-T087)

(function () {
  'use strict';

  const usersTbody = document.getElementById('users-tbody');
  const createUserButton = document.getElementById('create-user-button');
  const userModal = document.getElementById('user-modal');
  const userModalTitle = document.getElementById('user-modal-title');
  const userModalClose = document.getElementById('user-modal-close');
  const userModalCancel = document.getElementById('user-modal-cancel');
  const userModalSave = document.getElementById('user-modal-save');
  const userForm = document.getElementById('user-form');
  const userUsernameInput = document.getElementById('user-username');
  const userPasswordInput = document.getElementById('user-password');
  const userRoleSelect = document.getElementById('user-role');

  let users = [];
  let editingUserId = null;

  // ユーザー一覧を読み込む（T084）
  async function loadUsers() {
    try {
      const response = await authenticatedFetch('/api/users');
      if (response.ok) {
        users = await response.json();
        renderUsers();
      } else {
        showError('ユーザーの読み込みに失敗しました');
      }
    } catch (error) {
      console.error('Failed to load users:', error);
      showError('ユーザーの読み込みに失敗しました');
    }
  }

  // ユーザー一覧を表示（T084）
  function renderUsers() {
    if (users.length === 0) {
      usersTbody.innerHTML = '<tr><td colspan="5" class="empty-message">ユーザーがいません</td></tr>';
      return;
    }

    usersTbody.innerHTML = users
      .map((user) => {
        const createdAt = new Date(user.created_at).toLocaleString('ja-JP');
        const roleLabel = user.role === 'admin' ? 'Admin' : 'User';

        return `
          <tr>
            <td style="font-family: monospace; font-size: 0.85em;">${user.id.substring(0, 8)}...</td>
            <td>${escapeHtml(user.username)}</td>
            <td><span class="badge badge--${user.role}">${roleLabel}</span></td>
            <td>${createdAt}</td>
            <td>
              <button class="btn btn--secondary btn--small edit-user" data-id="${user.id}">編集</button>
              <button class="btn btn--danger btn--small delete-user" data-id="${user.id}" data-username="${escapeHtml(user.username)}">削除</button>
            </td>
          </tr>
        `;
      })
      .join('');

    // 編集・削除ボタンのイベントリスナーを追加
    document.querySelectorAll('.edit-user').forEach((btn) => {
      btn.addEventListener('click', function () {
        const userId = this.dataset.id;
        openEditUserModal(userId);
      });
    });

    document.querySelectorAll('.delete-user').forEach((btn) => {
      btn.addEventListener('click', function () {
        const userId = this.dataset.id;
        const username = this.dataset.username;
        deleteUser(userId, username);
      });
    });
  }

  // ユーザーを作成（T085）
  async function createUser() {
    const username = userUsernameInput.value.trim();
    const password = userPasswordInput.value;
    const role = userRoleSelect.value;

    if (!username || !password) {
      alert('ユーザー名とパスワードを入力してください');
      return;
    }

    try {
      const response = await authenticatedFetch('/api/users', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          username,
          password,
          role,
        }),
      });

      if (response.ok) {
        closeUserModal();
        loadUsers();
      } else {
        const error = await response.json().catch(() => ({}));
        alert(error.error || 'ユーザーの作成に失敗しました');
      }
    } catch (error) {
      console.error('Failed to create user:', error);
      alert('ユーザーの作成に失敗しました');
    }
  }

  // ユーザーを更新（T086: パスワード変更含む）
  async function updateUser(userId) {
    const username = userUsernameInput.value.trim();
    const password = userPasswordInput.value;
    const role = userRoleSelect.value;

    if (!username) {
      alert('ユーザー名を入力してください');
      return;
    }

    const body = {
      username,
      role,
    };

    // パスワードが入力されている場合のみ含める（T086）
    if (password) {
      body.password = password;
    }

    try {
      const response = await authenticatedFetch(`/api/users/${userId}`, {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        closeUserModal();
        loadUsers();
      } else {
        const error = await response.json().catch(() => ({}));
        alert(error.error || 'ユーザーの更新に失敗しました');
      }
    } catch (error) {
      console.error('Failed to update user:', error);
      alert('ユーザーの更新に失敗しました');
    }
  }

  // ユーザーを削除（T087: 最後の管理者警告）
  async function deleteUser(userId, username) {
    // 最後の管理者かチェック
    const adminCount = users.filter((u) => u.role === 'admin').length;
    const user = users.find((u) => u.id === userId);

    if (user && user.role === 'admin' && adminCount === 1) {
      alert('最後の管理者ユーザーは削除できません');
      return;
    }

    if (!confirm(`ユーザー "${username}" を削除しますか？`)) {
      return;
    }

    try {
      const response = await authenticatedFetch(`/api/users/${userId}`, {
        method: 'DELETE',
      });

      if (response.ok || response.status === 204) {
        loadUsers();
      } else {
        const error = await response.json().catch(() => ({}));
        alert(error.error || 'ユーザーの削除に失敗しました');
      }
    } catch (error) {
      console.error('Failed to delete user:', error);
      alert('ユーザーの削除に失敗しました');
    }
  }

  // ユーザー作成モーダルを開く
  function openCreateUserModal() {
    editingUserId = null;
    userModalTitle.textContent = 'ユーザーを作成';
    userForm.reset();
    userPasswordInput.required = true;
    userModal.classList.remove('hidden');
  }

  // ユーザー編集モーダルを開く
  function openEditUserModal(userId) {
    const user = users.find((u) => u.id === userId);
    if (!user) return;

    editingUserId = userId;
    userModalTitle.textContent = 'ユーザーを編集';
    userUsernameInput.value = user.username;
    userPasswordInput.value = '';
    userPasswordInput.required = false;
    userRoleSelect.value = user.role;
    userModal.classList.remove('hidden');
  }

  // ユーザーモーダルを閉じる
  function closeUserModal() {
    userModal.classList.add('hidden');
    userForm.reset();
    editingUserId = null;
  }

  // ユーザーモーダルの保存ボタン
  function handleUserModalSave() {
    if (editingUserId) {
      updateUser(editingUserId);
    } else {
      createUser();
    }
  }

  // エラーメッセージを表示
  function showError(message) {
    usersTbody.innerHTML = `<tr><td colspan="5" class="empty-message" style="color: #c53030;">${escapeHtml(message)}</td></tr>`;
  }

  // HTMLエスケープ
  function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  // イベントリスナー
  createUserButton.addEventListener('click', openCreateUserModal);
  userModalClose.addEventListener('click', closeUserModal);
  userModalCancel.addEventListener('click', closeUserModal);
  userModalSave.addEventListener('click', handleUserModalSave);

  // タブが開かれたときにユーザーを読み込む
  document.querySelectorAll('.tab-button').forEach((btn) => {
    btn.addEventListener('click', function () {
      if (this.dataset.tab === 'users') {
        loadUsers();
      }
    });
  });

  // 初期読み込み（ユーザータブがアクティブの場合）
  const currentTab = document.querySelector('.tab-button--active');
  if (currentTab && currentTab.dataset.tab === 'users') {
    loadUsers();
  }
})();
