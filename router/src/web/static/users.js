// User Management JavaScript (T084-T087)

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

  // Load user list (T084)
  async function loadUsers() {
    try {
      const response = await authenticatedFetch('/api/users');
      if (response.ok) {
        users = await response.json();
        renderUsers();
      } else {
        showError('Failed to load users');
      }
    } catch (error) {
      console.error('Failed to load users:', error);
      showError('Failed to load users');
    }
  }

  // Render user list (T084)
  function renderUsers() {
    if (users.length === 0) {
      usersTbody.innerHTML = '<tr><td colspan="5" class="empty-message">No users</td></tr>';
      return;
    }

    usersTbody.innerHTML = users
      .map((user) => {
        const createdAt = new Date(user.created_at).toLocaleString();
        const roleLabel = user.role === 'admin' ? 'Admin' : 'User';

        return `
          <tr>
            <td style="font-family: monospace; font-size: 0.85em;">${user.id.substring(0, 8)}...</td>
            <td>${escapeHtml(user.username)}</td>
            <td><span class="badge badge--${user.role}">${roleLabel}</span></td>
            <td>${createdAt}</td>
            <td>
              <button class="btn btn--secondary btn--small edit-user" data-id="${user.id}">Edit</button>
              <button class="btn btn--danger btn--small delete-user" data-id="${user.id}" data-username="${escapeHtml(user.username)}">Delete</button>
            </td>
          </tr>
        `;
      })
      .join('');

    // Add event listeners for edit/delete buttons
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

  // Create user (T085)
  async function createUser() {
    const username = userUsernameInput.value.trim();
    const password = userPasswordInput.value;
    const role = userRoleSelect.value;

    if (!username || !password) {
      alert('Please enter username and password');
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
        alert(error.error || 'Failed to create user');
      }
    } catch (error) {
      console.error('Failed to create user:', error);
      alert('Failed to create user');
    }
  }

  // Update user (T086: including password change)
  async function updateUser(userId) {
    const username = userUsernameInput.value.trim();
    const password = userPasswordInput.value;
    const role = userRoleSelect.value;

    if (!username) {
      alert('Please enter username');
      return;
    }

    const body = {
      username,
      role,
    };

    // Only include password if provided (T086)
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
        alert(error.error || 'Failed to update user');
      }
    } catch (error) {
      console.error('Failed to update user:', error);
      alert('Failed to update user');
    }
  }

  // Delete user (T087: last admin warning)
  async function deleteUser(userId, username) {
    // Check if last admin
    const adminCount = users.filter((u) => u.role === 'admin').length;
    const user = users.find((u) => u.id === userId);

    if (user && user.role === 'admin' && adminCount === 1) {
      alert('Cannot delete the last admin user');
      return;
    }

    if (!confirm(`Delete user "${username}"?`)) {
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
        alert(error.error || 'Failed to delete user');
      }
    } catch (error) {
      console.error('Failed to delete user:', error);
      alert('Failed to delete user');
    }
  }

  // Open create user modal
  function openCreateUserModal() {
    editingUserId = null;
    userModalTitle.textContent = 'Create User';
    userForm.reset();
    userPasswordInput.required = true;
    userModal.classList.remove('hidden');
  }

  // Open edit user modal
  function openEditUserModal(userId) {
    const user = users.find((u) => u.id === userId);
    if (!user) return;

    editingUserId = userId;
    userModalTitle.textContent = 'Edit User';
    userUsernameInput.value = user.username;
    userPasswordInput.value = '';
    userPasswordInput.required = false;
    userRoleSelect.value = user.role;
    userModal.classList.remove('hidden');
  }

  // Close user modal
  function closeUserModal() {
    userModal.classList.add('hidden');
    userForm.reset();
    editingUserId = null;
  }

  // User modal save button
  function handleUserModalSave() {
    if (editingUserId) {
      updateUser(editingUserId);
    } else {
      createUser();
    }
  }

  // Show error message
  function showError(message) {
    usersTbody.innerHTML = `<tr><td colspan="5" class="empty-message" style="color: #c53030;">${escapeHtml(message)}</td></tr>`;
  }

  // HTML escape
  function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  // Event listeners
  createUserButton.addEventListener('click', openCreateUserModal);
  userModalClose.addEventListener('click', closeUserModal);
  userModalCancel.addEventListener('click', closeUserModal);
  userModalSave.addEventListener('click', handleUserModalSave);

  // Load users when users tab is opened
  document.querySelectorAll('.tab-button').forEach((btn) => {
    btn.addEventListener('click', function () {
      if (this.dataset.tab === 'users') {
        loadUsers();
      }
    });
  });

  // Initial load (if users tab is active)
  const currentTab = document.querySelector('.tab-button--active');
  if (currentTab && currentTab.dataset.tab === 'users') {
    loadUsers();
  }
})();
