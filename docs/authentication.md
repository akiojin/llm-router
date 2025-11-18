# Authentication Guide

This document provides comprehensive information about Ollama Coordinator's
authentication and authorization system.

## Table of Contents

1. [Overview](#overview)
2. [Authentication Types](#authentication-types)
3. [Security Design](#security-design)
4. [Getting Started](#getting-started)
5. [User Management](#user-management)
6. [API Keys](#api-keys)
7. [Agent Authentication](#agent-authentication)
8. [Best Practices](#best-practices)
9. [Troubleshooting](#troubleshooting)

## Overview

Ollama Coordinator implements a multi-layered authentication system designed
to secure access to the coordinator, protect agent communication, and enable
external application integration.

### Authentication Layers

```
┌─────────────────────────────────────────────────────────────┐
│                     Client Applications                      │
│         (Dashboard, CLI tools, External apps)                │
└──────────────┬──────────────────┬───────────────────────────┘
               │                  │
         JWT Token          API Key (sk_*)
               │                  │
               ▼                  ▼
┌─────────────────────────────────────────────────────────────┐
│                    Coordinator Server                        │
│                                                               │
│  ┌────────────────────────────────────────────────────┐    │
│  │ Authentication Middleware                          │    │
│  │ - JWT Validator (Admin Users)                      │    │
│  │ - API Key Validator (External Apps)                │    │
│  │ - Agent Token Validator (Agents)                   │    │
│  └────────────────────────────────────────────────────┘    │
└──────────────┬──────────────────────────────────────────────┘
               │
         Agent Token (at_*)
               │
               ▼
┌─────────────────────────────────────────────────────────────┐
│                          Agents                              │
│           (Registered Ollama instances)                      │
└─────────────────────────────────────────────────────────────┘
```

### Authentication Types

| Type | Purpose | Token Format | Expiration | Storage |
|------|---------|--------------|------------|---------|
| **JWT** | Admin dashboard access | `eyJhbGci...` | 24 hours | Client-side |
| **API Key** | External app integration | `sk_...` | Configurable | App config |
| **Agent Token** | Agent authentication | `at_...` | Never | `~/.ollama-agent/token` |

## Authentication Types

### 1. JWT Authentication (Admin Users)

JWT (JSON Web Token) authentication is used for human administrators accessing
the web dashboard and management APIs.

#### Token Structure

```json
{
  "header": {
    "alg": "HS256",
    "typ": "JWT"
  },
  "payload": {
    "sub": "550e8400-e29b-41d4-a716-446655440000",
    "username": "admin",
    "role": "admin",
    "exp": 1700236800,
    "iat": 1700150400
  },
  "signature": "..."
}
```

#### User Roles

- **Admin**: Full access to all APIs and management functions
- **Viewer**: Read-only access (planned for future releases)

#### Token Lifecycle

1. **Login**: User provides username/password → Receives JWT token
2. **Request**: Client includes JWT in `Authorization: Bearer <token>` header
3. **Validation**: Coordinator verifies signature and expiration
4. **Expiration**: Token expires after 24 hours
5. **Renewal**: User must re-login to obtain a new token

### 2. API Key Authentication (External Applications)

API keys provide long-lived authentication for external applications,
scripts, and third-party integrations.

#### Key Format

- **Prefix**: `sk_` (secret key)
- **Length**: 32 characters (hexadecimal)
- **Example**: `sk_1234567890abcdef1234567890abcdef`

#### Key Properties

- **Name**: Human-readable identifier for the key
- **Expiration**: Optional expiration date (null = never expires)
- **Last Used**: Automatically updated on each use
- **User Association**: Each key belongs to a specific user

#### Supported Endpoints

API keys can be used for:

- OpenAI-compatible endpoints (`/v1/chat/completions`, `/v1/completions`)
- Ollama proxy endpoints (`/api/chat`, `/api/generate`)

**Note**: API keys cannot be used for administrative operations
(user management, API key management, etc.)

### 3. Agent Token Authentication (Agents)

Agent tokens secure communication between agents and the coordinator.

#### Token Format

- **Prefix**: `at_` (agent token)
- **Length**: 32 characters (hexadecimal)
- **Example**: `at_1234567890abcdef1234567890abcdef`

#### Token Lifecycle

1. **Registration**: Agent registers → Receives `agent_token` in response
2. **Storage**: Token saved to `~/.ollama-agent/token`
3. **Usage**: Token automatically included in all agent requests
4. **Re-registration**: Existing token used for re-registration
   (no new token issued)

#### Protected Endpoints

Agent tokens are required for:

- `POST /api/health` - Health check updates
- `POST /api/agents/:id/metrics` - Metrics reporting
- `POST /api/agents` - Re-registration (with existing token)

## Security Design

### Password Security

- **Algorithm**: bcrypt with cost factor 12
- **Salt**: Automatically generated per password
- **Storage**: Only password hashes stored (never plaintext)

### Token Security

#### JWT Tokens

- **Algorithm**: HS256 (HMAC-SHA256)
- **Secret**: Randomly generated on first startup (256-bit)
- **Expiration**: 24 hours from issuance
- **Claims**: User ID, username, role, issued-at, expiration

#### API Keys

- **Hashing**: SHA-256
- **Storage**: Only hash stored (plaintext key never persisted)
- **Generation**: Cryptographically secure random bytes
- **Prefix**: `sk_` for easy identification

#### Agent Tokens

- **Hashing**: SHA-256
- **Storage**: Only hash stored in coordinator database
- **Generation**: Cryptographically secure random bytes
- **Prefix**: `at_` for easy identification

### Database Security

```sql
-- Users table
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,  -- bcrypt hash
    role TEXT NOT NULL,
    created_at DATETIME NOT NULL
);

-- API Keys table
CREATE TABLE api_keys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    key_hash TEXT UNIQUE NOT NULL,  -- SHA-256 hash
    name TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    expires_at DATETIME,
    last_used_at DATETIME,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Agent Tokens table
CREATE TABLE agent_tokens (
    agent_id TEXT PRIMARY KEY,
    token_hash TEXT UNIQUE NOT NULL,  -- SHA-256 hash
    created_at DATETIME NOT NULL,
    FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
);
```

## Getting Started

### Initial Setup

On first startup, the coordinator prompts for admin account creation:

```bash
$ ./target/release/or-router

No admin users found. Please create the first admin account.
Username: admin
Password: ********
Confirm password: ********
Admin account created successfully!

Coordinator starting on http://0.0.0.0:8080
```

### First Login

```bash
# Login and receive JWT token
curl -X POST http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username": "admin",
    "password": "your-password"
  }'
```

**Response:**

```json
{
  "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "user": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "username": "admin",
    "role": "admin",
    "created_at": "2025-11-17T10:00:00Z"
  }
}
```

### Using the Dashboard

1. Navigate to `http://localhost:8080/dashboard`
2. Login with your credentials
3. JWT token is automatically stored in browser session
4. Token is included in all dashboard API requests

## User Management

### Creating Additional Users

```bash
TOKEN="your-jwt-token"

curl -X POST http://localhost:8080/api/users \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "username": "developer",
    "password": "secure-password",
    "role": "admin"
  }'
```

### Listing Users

```bash
curl http://localhost:8080/api/users \
  -H "Authorization: Bearer $TOKEN"
```

### Updating User Password

```bash
curl -X PUT http://localhost:8080/api/users/{user_id} \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "password": "new-secure-password"
  }'
```

### Deleting Users

```bash
curl -X DELETE http://localhost:8080/api/users/{user_id} \
  -H "Authorization: Bearer $TOKEN"
```

**Warning**: Deleting a user also deletes all associated API keys.

## API Keys

### Creating an API Key

```bash
TOKEN="your-jwt-token"

# Create API key without expiration
curl -X POST http://localhost:8080/api/api-keys \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Production API Key"
  }'

# Create API key with expiration
curl -X POST http://localhost:8080/api/api-keys \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Temporary Key",
    "expires_at": "2025-12-31T23:59:59Z"
  }'
```

**Response:**

```json
{
  "id": "770ea622-g49d-63f6-d938-668877662222",
  "key": "sk_1234567890abcdef1234567890abcdef",
  "name": "Production API Key",
  "created_at": "2025-11-17T10:00:00Z",
  "expires_at": null
}
```

**Important**: The plaintext `key` is only returned once. Store it securely.

### Using an API Key

```bash
API_KEY="sk_1234567890abcdef1234567890abcdef"

# OpenAI-compatible endpoint
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama2",
    "messages": [
      {"role": "user", "content": "Hello!"}
    ]
  }'

# Ollama endpoint
curl -X POST http://localhost:8080/api/chat \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama2",
    "messages": [
      {"role": "user", "content": "Hello!"}
    ],
    "stream": false
  }'
```

### Listing API Keys

```bash
curl http://localhost:8080/api/api-keys \
  -H "Authorization: Bearer $TOKEN"
```

**Response:**

```json
[
  {
    "id": "770ea622-g49d-63f6-d938-668877662222",
    "name": "Production API Key",
    "created_at": "2025-11-17T10:00:00Z",
    "expires_at": null,
    "last_used_at": "2025-11-17T14:30:00Z"
  }
]
```

**Note**: The plaintext key is never returned in list responses.

### Revoking an API Key

```bash
curl -X DELETE http://localhost:8080/api/api-keys/{key_id} \
  -H "Authorization: Bearer $TOKEN"
```

**Effect**: The API key is immediately invalidated and cannot be used.

## Agent Authentication

### Agent Registration

When an agent registers, it receives an authentication token:

**Request:**

```bash
curl -X POST http://localhost:8080/api/agents \
  -H "Content-Type: application/json" \
  -d '{
    "machine_name": "my-machine",
    "ip_address": "192.168.1.100",
    "ollama_version": "0.1.0",
    "ollama_port": 11434,
    "gpu_available": true,
    "gpu_devices": [
      {"model": "NVIDIA RTX 4090", "count": 2}
    ]
  }'
```

**Response:**

```json
{
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",
  "agent_token": "at_1234567890abcdef1234567890abcdef",
  "status": "registered"
}
```

### Token Storage

The agent automatically saves the token to:

- **Linux/macOS**: `~/.ollama-agent/token`
- **Windows**: `%USERPROFILE%\.ollama-agent\token`

### Using Agent Token

The agent includes the token in all requests:

```bash
AGENT_TOKEN="at_1234567890abcdef1234567890abcdef"

# Health check
curl -X POST http://localhost:8080/api/health \
  -H "X-Agent-Token: $AGENT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "agent_id": "550e8400-e29b-41d4-a716-446655440000",
    "cpu_usage": 45.5,
    "memory_usage": 60.2,
    "active_requests": 3
  }'
```

### Agent Re-registration

When an agent restarts, it uses the existing token:

**Request:**

```bash
curl -X POST http://localhost:8080/api/agents \
  -H "X-Agent-Token: at_1234567890abcdef1234567890abcdef" \
  -H "Content-Type: application/json" \
  -d '{
    "machine_name": "my-machine",
    ...
  }'
```

**Response:**

```json
{
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",
  "agent_token": null,
  "status": "registered"
}
```

**Note**: No new token is issued for re-registration.

## Best Practices

### Password Management

1. **Use Strong Passwords**
   - Minimum 12 characters
   - Mix of uppercase, lowercase, numbers, and symbols
   - Avoid common words and patterns

2. **Rotate Passwords Regularly**
   - Change admin passwords every 90 days
   - Update immediately if compromise suspected

3. **Never Share Credentials**
   - Create separate accounts for each administrator
   - Use API keys for application access

### API Key Management

1. **Use Descriptive Names**
   - Include purpose and owner: `production-api-chatbot-team`
   - Makes auditing and rotation easier

2. **Set Expiration Dates**
   - Use expiration for temporary or test keys
   - Review and rotate long-lived keys regularly

3. **Scope Appropriately**
   - Create separate keys for different applications
   - Easier to revoke compromised keys without affecting others

4. **Secure Storage**
   - Store keys in environment variables or secret managers
   - Never commit keys to version control
   - Use `.env` files with `.gitignore`

5. **Monitor Usage**
   - Check `last_used_at` timestamps regularly
   - Revoke unused keys

6. **Rotation Strategy**
   - Rotate keys every 6-12 months
   - Create new key before revoking old key (zero-downtime rotation)

### Agent Token Security

1. **File Permissions**
   - Agent token file should be readable only by agent user
   - `chmod 600 ~/.ollama-agent/token` (Linux/macOS)

2. **Network Security**
   - Use HTTPS/TLS for production deployments
   - Isolate agent network traffic when possible

3. **Token Rotation**
   - Delete agent from coordinator to invalidate token
   - Re-register agent to receive new token

### Production Deployment

1. **Use HTTPS**
   - Deploy coordinator behind reverse proxy with TLS
   - Recommended: nginx, Caddy, or Traefik

2. **Set Strong JWT Secret**
   - Override default with `JWT_SECRET` environment variable
   - Use at least 256 bits of entropy

3. **Enable Rate Limiting**
   - Protect against brute-force attacks
   - Implement at reverse proxy level

4. **Regular Backups**
   - Backup coordinator database regularly
   - Includes users, API keys (hashes), and agent tokens (hashes)

5. **Audit Logging**
   - Monitor authentication failures
   - Track API key usage patterns

## Troubleshooting

### Common Issues

#### "Unauthorized" Error

**Symptoms:**

```json
{
  "error": "Unauthorized"
}
```

**Causes:**

1. **Missing Token**: No `Authorization` header provided
2. **Invalid Token**: Token is malformed or corrupted
3. **Expired Token**: JWT token has expired (24 hours)
4. **Wrong Token Type**: Using API key for admin endpoint or vice versa

**Solutions:**

```bash
# Verify token is included in request
curl http://localhost:8080/api/users \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -v

# Check token expiration (decode JWT at jwt.io)
# Re-login to get new token
curl -X POST http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username": "admin", "password": "your-password"}'
```

#### "Invalid credentials" Error

**Symptoms:**

```json
{
  "error": "Invalid credentials"
}
```

**Causes:**

1. Wrong username or password
2. User account deleted

**Solutions:**

1. Verify credentials are correct
2. Check if user exists in database
3. Reset password if forgotten (requires database access)

#### Agent Registration Fails

**Symptoms:**

- Agent cannot register with coordinator
- "Unauthorized" error during health checks

**Solutions:**

1. **Delete token file and re-register:**

   ```bash
   rm ~/.ollama-agent/token
   ./or-node
   ```

2. **Verify coordinator URL:**

   ```bash
   echo $COORDINATOR_URL
   # Should be http://coordinator:8080 (no trailing slash)
   ```

3. **Check network connectivity:**

   ```bash
   curl http://coordinator:8080/api/agents
   ```

#### API Key Not Working

**Symptoms:**

- "Unauthorized" when using API key
- Works in one endpoint but not another

**Causes:**

1. API key expired
2. API key deleted
3. Using API key for admin endpoint (not supported)

**Solutions:**

1. **Check API key status:**

   ```bash
   curl http://localhost:8080/api/api-keys \
     -H "Authorization: Bearer $JWT_TOKEN"
   ```

2. **Verify endpoint supports API keys:**
   - ✅ `/v1/chat/completions`
   - ✅ `/api/chat`
   - ❌ `/api/users` (requires JWT)

3. **Create new API key if deleted:**

   ```bash
   curl -X POST http://localhost:8080/api/api-keys \
     -H "Authorization: Bearer $JWT_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name": "New API Key"}'
   ```

### Debugging Authentication

#### Enable Debug Logging

Set environment variable:

```bash
RUST_LOG=debug ./or-router
```

Look for authentication-related logs:

```
[DEBUG] Validating JWT token
[DEBUG] Token signature verified
[DEBUG] User authenticated: admin (550e8400-e29b-41d4-a716-446655440000)
```

#### Verify Database State

```bash
# Connect to SQLite database
sqlite3 ~/.or/router.db

-- List users
SELECT id, username, role, created_at FROM users;

-- List API keys
SELECT id, name, created_at, expires_at FROM api_keys;

-- List agent tokens
SELECT agent_id, created_at FROM agent_tokens;
```

#### Test Authentication Manually

```bash
# 1. Login
TOKEN=$(curl -s -X POST http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username": "admin", "password": "your-password"}' \
  | jq -r '.token')

# 2. Verify token
curl http://localhost:8080/api/auth/me \
  -H "Authorization: Bearer $TOKEN"

# 3. List users (admin endpoint)
curl http://localhost:8080/api/users \
  -H "Authorization: Bearer $TOKEN"
```

### Resetting Authentication

#### Reset Admin Password (Database Access Required)

```bash
# WARNING: This requires direct database access and Rust tooling

# 1. Generate new password hash
echo -n "new-password" | cargo run -p ollama-coordinator-common --bin hash-password

# Output: $2b$12$...

# 2. Update database
sqlite3 ~/.or/router.db
UPDATE users SET password_hash = '$2b$12$...' WHERE username = 'admin';
```

#### Reset JWT Secret

```bash
# Delete existing JWT secret
rm ~/.or/jwt_secret

# Restart coordinator (generates new secret)
./or-router

# All existing JWT tokens are now invalid
# Users must re-login
```

**Warning**: This invalidates all existing JWT tokens.

#### Clear All API Keys

```bash
sqlite3 ~/.or/router.db
DELETE FROM api_keys;
```

**Warning**: This revokes all API keys. External applications will stop working.

## Security Considerations

### Known Limitations

1. **No Token Revocation List**
   - JWT tokens cannot be invalidated before expiration
   - Logout does not blacklist tokens
   - Workaround: Use short expiration times (24 hours)

2. **No Refresh Tokens**
   - Users must re-login after token expiration
   - Future: Implement refresh token mechanism

3. **No Rate Limiting (Built-in)**
   - Vulnerable to brute-force attacks without reverse proxy
   - Recommendation: Deploy behind nginx/Caddy with rate limiting

4. **No 2FA/MFA**
   - Only username/password authentication
   - Future: Add TOTP support

### Reporting Security Issues

If you discover a security vulnerability, please email:

**<security@your-domain.com>**

Please do not create public GitHub issues for security vulnerabilities.

## See Also

- [README.md](../README.md) - Main project documentation
- [Dashboard Guide](./dashboard.md) - Web dashboard documentation
- [API Specification](../README.md#api-specification) - Complete API reference
