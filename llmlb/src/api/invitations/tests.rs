use super::*;

#[test]
fn test_create_invitation_request_deserialize() {
    let json = r#"{"expires_in_hours": 48}"#;
    let request: CreateInvitationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.expires_in_hours, Some(48));
}

#[test]
fn test_create_invitation_request_default() {
    let json = r#"{}"#;
    let request: CreateInvitationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.expires_in_hours, None);
}

#[test]
fn test_create_invitation_response_serialize() {
    let response = CreateInvitationResponse {
        id: "test-id".to_string(),
        code: "inv_abcd1234".to_string(),
        created_at: "2025-12-20T15:00:00Z".to_string(),
        expires_at: "2025-12-23T15:00:00Z".to_string(),
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("inv_abcd1234"));
    assert!(json.contains("test-id"));
}

#[test]
fn test_invitation_response_serialize() {
    let response = InvitationResponse {
        id: "id-123".to_string(),
        created_by: "admin-id".to_string(),
        created_at: "2025-12-20T15:00:00Z".to_string(),
        expires_at: "2025-12-23T15:00:00Z".to_string(),
        status: "active".to_string(),
        used_by: None,
        used_at: None,
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("active"));
    assert!(json.contains("admin-id"));
}

#[test]
fn test_invitation_response_with_used() {
    let response = InvitationResponse {
        id: "id-456".to_string(),
        created_by: "admin-id".to_string(),
        created_at: "2025-12-20T15:00:00Z".to_string(),
        expires_at: "2025-12-23T15:00:00Z".to_string(),
        status: "used".to_string(),
        used_by: Some("user-id".to_string()),
        used_at: Some("2025-12-21T10:00:00Z".to_string()),
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("used"));
    assert!(json.contains("user-id"));
}

#[test]
fn test_list_invitations_response_serialize() {
    let response = ListInvitationsResponse {
        invitations: vec![InvitationResponse {
            id: "id-1".to_string(),
            created_by: "admin".to_string(),
            created_at: "2025-12-20T15:00:00Z".to_string(),
            expires_at: "2025-12-23T15:00:00Z".to_string(),
            status: "active".to_string(),
            used_by: None,
            used_at: None,
        }],
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("invitations"));
    assert!(json.contains("id-1"));
}

// --- CreateInvitationRequest additional tests ---

#[test]
fn test_create_invitation_request_with_zero_hours() {
    let json = r#"{"expires_in_hours": 0}"#;
    let request: CreateInvitationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.expires_in_hours, Some(0));
}

#[test]
fn test_create_invitation_request_with_negative_hours() {
    let json = r#"{"expires_in_hours": -1}"#;
    let request: CreateInvitationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.expires_in_hours, Some(-1));
}

#[test]
fn test_create_invitation_request_with_large_hours() {
    let json = r#"{"expires_in_hours": 87600}"#;
    let request: CreateInvitationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.expires_in_hours, Some(87600));
}

#[test]
fn test_create_invitation_request_rejects_string_hours() {
    let json = r#"{"expires_in_hours": "48"}"#;
    let result = serde_json::from_str::<CreateInvitationRequest>(json);
    assert!(result.is_err());
}

#[test]
fn test_create_invitation_request_ignores_extra_fields() {
    let json = r#"{"expires_in_hours": 24, "unknown_field": "value"}"#;
    let request: CreateInvitationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.expires_in_hours, Some(24));
}

#[test]
fn test_create_invitation_request_null_hours_is_none() {
    let json = r#"{"expires_in_hours": null}"#;
    let request: CreateInvitationRequest = serde_json::from_str(json).unwrap();
    assert!(request.expires_in_hours.is_none());
}

// --- CreateInvitationResponse additional tests ---

#[test]
fn test_create_invitation_response_all_fields_present() {
    let response = CreateInvitationResponse {
        id: "uuid-id".to_string(),
        code: "inv_test123".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        expires_at: "2026-01-04T00:00:00Z".to_string(),
    };
    let json: serde_json::Value = serde_json::to_value(&response).unwrap();
    assert_eq!(json["id"], "uuid-id");
    assert_eq!(json["code"], "inv_test123");
    assert_eq!(json["created_at"], "2026-01-01T00:00:00Z");
    assert_eq!(json["expires_at"], "2026-01-04T00:00:00Z");
}

#[test]
fn test_create_invitation_response_empty_code() {
    let response = CreateInvitationResponse {
        id: "id".to_string(),
        code: "".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        expires_at: "2026-01-04T00:00:00Z".to_string(),
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"code\":\"\""));
}

// --- InvitationResponse additional tests ---

#[test]
fn test_invitation_response_revoked_status() {
    let response = InvitationResponse {
        id: "id-789".to_string(),
        created_by: "admin-id".to_string(),
        created_at: "2025-12-20T15:00:00Z".to_string(),
        expires_at: "2025-12-23T15:00:00Z".to_string(),
        status: "revoked".to_string(),
        used_by: None,
        used_at: None,
    };
    let json: serde_json::Value = serde_json::to_value(&response).unwrap();
    assert_eq!(json["status"], "revoked");
    assert!(json["used_by"].is_null());
    assert!(json["used_at"].is_null());
}

#[test]
fn test_invitation_response_all_fields_populated() {
    let response = InvitationResponse {
        id: "id-full".to_string(),
        created_by: "admin-uuid".to_string(),
        created_at: "2025-12-20T15:00:00Z".to_string(),
        expires_at: "2025-12-23T15:00:00Z".to_string(),
        status: "used".to_string(),
        used_by: Some("user-uuid".to_string()),
        used_at: Some("2025-12-21T10:00:00Z".to_string()),
    };
    let json: serde_json::Value = serde_json::to_value(&response).unwrap();
    assert_eq!(json["used_by"], "user-uuid");
    assert_eq!(json["used_at"], "2025-12-21T10:00:00Z");
}

#[test]
fn test_invitation_response_field_count() {
    let response = InvitationResponse {
        id: "id".to_string(),
        created_by: "admin".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        expires_at: "2025-01-02T00:00:00Z".to_string(),
        status: "active".to_string(),
        used_by: None,
        used_at: None,
    };
    let json: serde_json::Value = serde_json::to_value(&response).unwrap();
    let obj = json.as_object().unwrap();
    assert_eq!(obj.len(), 7); // id, created_by, created_at, expires_at, status, used_by, used_at
}

// --- ListInvitationsResponse additional tests ---

#[test]
fn test_list_invitations_response_empty() {
    let response = ListInvitationsResponse {
        invitations: vec![],
    };
    let json: serde_json::Value = serde_json::to_value(&response).unwrap();
    assert!(json["invitations"].as_array().unwrap().is_empty());
}

#[test]
fn test_list_invitations_response_multiple_items() {
    let response = ListInvitationsResponse {
        invitations: vec![
            InvitationResponse {
                id: "id-1".to_string(),
                created_by: "admin".to_string(),
                created_at: "2025-12-20T15:00:00Z".to_string(),
                expires_at: "2025-12-23T15:00:00Z".to_string(),
                status: "active".to_string(),
                used_by: None,
                used_at: None,
            },
            InvitationResponse {
                id: "id-2".to_string(),
                created_by: "admin".to_string(),
                created_at: "2025-12-21T15:00:00Z".to_string(),
                expires_at: "2025-12-24T15:00:00Z".to_string(),
                status: "used".to_string(),
                used_by: Some("user1".to_string()),
                used_at: Some("2025-12-22T10:00:00Z".to_string()),
            },
            InvitationResponse {
                id: "id-3".to_string(),
                created_by: "admin".to_string(),
                created_at: "2025-12-22T15:00:00Z".to_string(),
                expires_at: "2025-12-25T15:00:00Z".to_string(),
                status: "revoked".to_string(),
                used_by: None,
                used_at: None,
            },
        ],
    };
    let json: serde_json::Value = serde_json::to_value(&response).unwrap();
    let arr = json["invitations"].as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["status"], "active");
    assert_eq!(arr[1]["status"], "used");
    assert_eq!(arr[2]["status"], "revoked");
}

// --- serde roundtrip tests ---

#[test]
fn test_create_invitation_request_deserialize_with_value() {
    let json = r#"{"expires_in_hours": 72}"#;
    let req: CreateInvitationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.expires_in_hours, Some(72));
}

#[test]
fn test_create_invitation_response_serde_roundtrip() {
    let original = CreateInvitationResponse {
        id: "test-id".to_string(),
        code: "inv_abc".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        expires_at: "2026-01-04T00:00:00Z".to_string(),
    };
    let json = serde_json::to_string(&original).unwrap();
    // CreateInvitationResponse is Serialize-only, so verify the JSON structure
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["id"], "test-id");
    assert_eq!(value["code"], "inv_abc");
}

#[test]
fn test_invitation_response_serde_roundtrip() {
    let original = InvitationResponse {
        id: "id-rt".to_string(),
        created_by: "admin-rt".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        expires_at: "2026-01-04T00:00:00Z".to_string(),
        status: "active".to_string(),
        used_by: None,
        used_at: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["id"], "id-rt");
    assert_eq!(value["status"], "active");
}

#[test]
fn test_list_invitations_response_serde_roundtrip() {
    let original = ListInvitationsResponse {
        invitations: vec![InvitationResponse {
            id: "id-rt".to_string(),
            created_by: "admin".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            expires_at: "2026-01-04T00:00:00Z".to_string(),
            status: "active".to_string(),
            used_by: None,
            used_at: None,
        }],
    };
    let json = serde_json::to_string(&original).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["invitations"].as_array().unwrap().len(), 1);
}
