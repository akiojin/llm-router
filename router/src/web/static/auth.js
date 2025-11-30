// Authentication utility functions

/**
 * Wrapper function to execute fetch with JWT token
 * @param {string} url - Request URL
 * @param {RequestInit} options - fetch options
 * @returns {Promise<Response>} - fetch response
 */
async function authenticatedFetch(url, options = {}) {
  const token = localStorage.getItem('jwt_token');

  const headers = {
    ...options.headers,
  };

  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }

  const response = await fetch(url, {
    ...options,
    headers,
  });

  // Redirect to login page on 401
  if (response.status === 401) {
    localStorage.removeItem('jwt_token');
    window.location.href = '/dashboard/login.html';
    return response;
  }

  return response;
}

// Expose globally
window.authenticatedFetch = authenticatedFetch;
