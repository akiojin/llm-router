//! OpenAPI スペック解決: ファイル読込（cwd 祖先探索）またはデフォルト spec 返却
//!
//! arch-review [H6] round2: cli/assistant.rs から OpenAPI 解決を分離。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(super) fn load_openapi_value(path: Option<&PathBuf>, env_path: Option<&PathBuf>) -> Value {
    let mut candidates = Vec::new();

    if let Some(path) = path {
        candidates.push(path.clone());
    }

    if let Some(path) = env_path {
        candidates.push(path.clone());
    }

    // Backward-compatible default: search docs/openapi.yaml from cwd to ancestors.
    if candidates.is_empty() {
        if let Some(path) = find_openapi_in_ancestors(
            std::env::current_dir()
                .ok()
                .as_deref()
                .unwrap_or(Path::new(".")),
        ) {
            candidates.push(path);
        }
    }

    for candidate in candidates {
        if candidate.exists() {
            if let Ok(content) = std::fs::read_to_string(&candidate) {
                if let Ok(json_value) = serde_json::from_str::<Value>(&content) {
                    return json_value;
                }

                if let Ok(yaml_value) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                    if let Ok(json_value) = serde_json::to_value(yaml_value) {
                        return json_value;
                    }
                }
            }
        }
    }

    default_openapi_spec()
}

pub(super) fn find_openapi_in_ancestors(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let candidate = dir.join("docs").join("openapi.yaml");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

pub(super) fn default_openapi_spec() -> Value {
    json!({
      "openapi": "3.1.0",
      "info": {
        "title": "llmlb API",
        "version": "0.1.0",
        "description": "OpenAI-compatible endpoints with optional cloud routing by model prefix (openai:/google:/anthropic:)."
      },
      "servers": [{ "url": "http://localhost:32768" }],
      "paths": {
        "/v1/chat/completions": {
          "post": {
            "summary": "Chat completion (local or cloud depending on model prefix)",
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/ChatRequest" }
                }
              }
            },
            "responses": {
              "200": {
                "description": "Chat completion response",
                "content": {
                  "application/json": {
                    "schema": { "$ref": "#/components/schemas/ChatResponse" }
                  }
                }
              }
            }
          }
        },
        "/v1/models": {
          "get": {
            "summary": "List available models",
            "responses": {
              "200": { "description": "List of models" }
            }
          }
        },
        "/v1/embeddings": {
          "post": {
            "summary": "Generate embeddings",
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/EmbeddingRequest" }
                }
              }
            }
          }
        },
        "/api/auth/login": {
          "post": { "summary": "Login (sets HttpOnly cookie for the dashboard)" }
        },
        "/api/auth/me": {
          "get": { "summary": "Get current user session" }
        },
        "/api/endpoints": {
          "get": { "summary": "List endpoints" },
          "post": { "summary": "Create endpoint" }
        },
        "/api/endpoints/{id}": {
          "get": { "summary": "Get endpoint detail" },
          "put": { "summary": "Update endpoint" },
          "delete": { "summary": "Delete endpoint" }
        },
        "/api/dashboard/overview": {
          "get": { "summary": "Get dashboard overview" }
        },
        "/api/dashboard/stats": {
          "get": { "summary": "Get dashboard statistics" }
        },
        "/api/models/register": {
          "post": { "summary": "Register a model (admin only)" }
        }
      },
      "components": {
        "schemas": {
          "ChatRequest": {
            "type": "object",
            "properties": {
              "model": { "type": "string", "example": "openai:gpt-4o" },
              "messages": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/ChatMessage" }
              },
              "stream": { "type": "boolean" }
            },
            "required": ["model", "messages"]
          },
          "ChatMessage": {
            "type": "object",
            "properties": {
              "role": { "type": "string", "enum": ["system", "user", "assistant"] },
              "content": { "type": "string" }
            },
            "required": ["role", "content"]
          },
          "ChatResponse": {
            "type": "object",
            "properties": {
              "id": { "type": "string" },
              "model": { "type": "string" },
              "choices": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/ChatChoice" }
              }
            }
          },
          "ChatChoice": {
            "type": "object",
            "properties": {
              "index": { "type": "integer" },
              "message": { "$ref": "#/components/schemas/ChatMessage" },
              "finish_reason": { "type": "string" }
            }
          },
          "EmbeddingRequest": {
            "type": "object",
            "properties": {
              "model": { "type": "string" },
              "input": { "oneOf": [{ "type": "string" }, { "type": "array" }] }
            },
            "required": ["model", "input"]
          }
        }
      }
    })
}
