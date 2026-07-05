use super::*;

#[tokio::test]
async fn test_event_bus_publish_subscribe() {
    let bus = DashboardEventBus::new();
    let mut receiver = bus.subscribe();

    let event = DashboardEvent::EndpointRegistered {
        endpoint_id: Uuid::new_v4(),
        endpoint_name: "test-node".to_string(),
        ip_address: "127.0.0.1".to_string(),
        status: EndpointStatus::Online,
    };

    bus.publish(event.clone());

    let received = receiver.recv().await.unwrap();
    match received {
        DashboardEvent::EndpointRegistered { endpoint_name, .. } => {
            assert_eq!(endpoint_name, "test-node");
        }
        _ => panic!("Unexpected event type"),
    }
}

#[test]
fn test_event_bus_no_subscribers() {
    let bus = DashboardEventBus::new();

    // 購読者がいなくてもパニックしないことを確認
    bus.publish(DashboardEvent::EndpointRemoved {
        endpoint_id: Uuid::new_v4(),
    });
}

#[test]
fn test_subscriber_count() {
    let bus = DashboardEventBus::new();
    assert_eq!(bus.subscriber_count(), 0);

    let _r1 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 1);

    let _r2 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 2);
}

// T017: DashboardEvent::TpsUpdated シリアライゼーションテスト（SPEC-4bb5b55f Phase 4）

#[test]
fn test_tps_updated_event_serialization() {
    let endpoint_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
    let event = DashboardEvent::TpsUpdated {
        endpoint_id,
        model_id: "llama3.2:3b".to_string(),
        tps: 42.5,
        output_tokens: 100,
        duration_ms: 2353,
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "TpsUpdated");
    let data = &json["data"];
    assert_eq!(data["endpoint_id"], "12345678-1234-1234-1234-123456789abc");
    assert_eq!(data["model_id"], "llama3.2:3b");
    assert!((data["tps"].as_f64().unwrap() - 42.5).abs() < 0.01);
    assert_eq!(data["output_tokens"], 100);
    assert_eq!(data["duration_ms"], 2353);
}

#[test]
fn test_update_state_changed_event_serialization() {
    let event = DashboardEvent::UpdateStateChanged;

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "UpdateStateChanged");
}

#[tokio::test]
async fn test_update_state_changed_event_broadcast() {
    let bus = DashboardEventBus::new();
    let mut receiver = bus.subscribe();

    bus.publish(DashboardEvent::UpdateStateChanged);

    let received = receiver.recv().await.unwrap();
    match received {
        DashboardEvent::UpdateStateChanged => {}
        _ => panic!("Expected UpdateStateChanged event"),
    }
}

#[tokio::test]
async fn test_tps_updated_event_broadcast() {
    let bus = DashboardEventBus::new();
    let mut receiver = bus.subscribe();

    bus.publish(DashboardEvent::TpsUpdated {
        endpoint_id: Uuid::new_v4(),
        model_id: "test-model".to_string(),
        tps: 50.0,
        output_tokens: 200,
        duration_ms: 4000,
    });

    let received = receiver.recv().await.unwrap();
    match received {
        DashboardEvent::TpsUpdated { model_id, tps, .. } => {
            assert_eq!(model_id, "test-model");
            assert!((tps - 50.0).abs() < 0.01);
        }
        _ => panic!("Expected TpsUpdated event"),
    }
}

// --- DashboardEventBus additional tests ---

#[test]
fn test_event_bus_default() {
    let bus = DashboardEventBus::default();
    assert_eq!(bus.subscriber_count(), 0);
}

#[test]
fn test_subscriber_count_decreases_on_drop() {
    let bus = DashboardEventBus::new();
    let r1 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 1);
    drop(r1);
    assert_eq!(bus.subscriber_count(), 0);
}

#[test]
fn test_create_shared_event_bus() {
    let shared = create_shared_event_bus();
    assert_eq!(shared.subscriber_count(), 0);
    let _r = shared.subscribe();
    assert_eq!(shared.subscriber_count(), 1);
}

#[test]
fn test_shared_event_bus_clone() {
    let shared = create_shared_event_bus();
    let shared2 = shared.clone();
    let _r = shared.subscribe();
    // Cloned bus sees same subscribers
    assert_eq!(shared2.subscriber_count(), 1);
}

#[test]
fn test_event_bus_clone_shares_channel() {
    let bus1 = DashboardEventBus::new();
    let bus2 = bus1.clone();
    let _r = bus1.subscribe();
    assert_eq!(bus2.subscriber_count(), 1);
}

// --- DashboardEvent serialization tests ---

#[test]
fn test_node_registered_event_serialization() {
    let id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
    let event = DashboardEvent::EndpointRegistered {
        endpoint_id: id,
        endpoint_name: "gpu-server-01".to_string(),
        ip_address: "192.168.1.100".to_string(),
        status: EndpointStatus::Online,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "NodeRegistered");
    let data = &json["data"];
    assert_eq!(data["runtime_id"], "12345678-1234-1234-1234-123456789abc");
    assert_eq!(data["machine_name"], "gpu-server-01");
    assert_eq!(data["ip_address"], "192.168.1.100");
    assert_eq!(data["status"], "online");
}

#[test]
fn test_endpoint_status_changed_event_serialization() {
    let id = Uuid::parse_str("abcdef12-3456-7890-abcd-ef1234567890").unwrap();
    let event = DashboardEvent::EndpointStatusChanged {
        endpoint_id: id,
        old_status: EndpointStatus::Online,
        new_status: EndpointStatus::Offline,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "EndpointStatusChanged");
    let data = &json["data"];
    assert_eq!(data["old_status"], "online");
    assert_eq!(data["new_status"], "offline");
}

#[test]
fn test_metrics_updated_event_serialization_full() {
    let id = Uuid::new_v4();
    let event = DashboardEvent::MetricsUpdated {
        endpoint_id: id,
        cpu_usage: Some(75.5),
        memory_usage: Some(60.0),
        gpu_usage: Some(90.0),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "MetricsUpdated");
    let data = &json["data"];
    assert!((data["cpu_usage"].as_f64().unwrap() - 75.5).abs() < 0.01);
    assert!((data["memory_usage"].as_f64().unwrap() - 60.0).abs() < 0.01);
    assert!((data["gpu_usage"].as_f64().unwrap() - 90.0).abs() < 0.01);
}

#[test]
fn test_metrics_updated_event_serialization_with_nulls() {
    let id = Uuid::new_v4();
    let event = DashboardEvent::MetricsUpdated {
        endpoint_id: id,
        cpu_usage: None,
        memory_usage: None,
        gpu_usage: None,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "MetricsUpdated");
    let data = &json["data"];
    assert!(data["cpu_usage"].is_null());
    assert!(data["memory_usage"].is_null());
    assert!(data["gpu_usage"].is_null());
}

#[test]
fn test_node_removed_event_serialization() {
    let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
    let event = DashboardEvent::EndpointRemoved { endpoint_id: id };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "NodeRemoved");
    assert_eq!(
        json["data"]["runtime_id"],
        "11111111-2222-3333-4444-555555555555"
    );
}

// --- Multiple subscriber broadcast tests ---

#[tokio::test]
async fn test_multiple_subscribers_receive_same_event() {
    let bus = DashboardEventBus::new();
    let mut r1 = bus.subscribe();
    let mut r2 = bus.subscribe();
    let mut r3 = bus.subscribe();

    bus.publish(DashboardEvent::UpdateStateChanged);

    let e1 = r1.recv().await.unwrap();
    let e2 = r2.recv().await.unwrap();
    let e3 = r3.recv().await.unwrap();

    assert!(matches!(e1, DashboardEvent::UpdateStateChanged));
    assert!(matches!(e2, DashboardEvent::UpdateStateChanged));
    assert!(matches!(e3, DashboardEvent::UpdateStateChanged));
}

#[tokio::test]
async fn test_multiple_events_in_sequence() {
    let bus = DashboardEventBus::new();
    let mut receiver = bus.subscribe();

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();

    bus.publish(DashboardEvent::EndpointRemoved { endpoint_id: id1 });
    bus.publish(DashboardEvent::EndpointRemoved { endpoint_id: id2 });

    let e1 = receiver.recv().await.unwrap();
    let e2 = receiver.recv().await.unwrap();

    match e1 {
        DashboardEvent::EndpointRemoved { endpoint_id } => assert_eq!(endpoint_id, id1),
        _ => panic!("Expected NodeRemoved"),
    }
    match e2 {
        DashboardEvent::EndpointRemoved { endpoint_id } => assert_eq!(endpoint_id, id2),
        _ => panic!("Expected NodeRemoved"),
    }
}

#[tokio::test]
async fn test_node_registered_event_broadcast_fields() {
    let bus = DashboardEventBus::new();
    let mut receiver = bus.subscribe();

    let id = Uuid::new_v4();
    bus.publish(DashboardEvent::EndpointRegistered {
        endpoint_id: id,
        endpoint_name: "test-machine".to_string(),
        ip_address: "10.0.0.1".to_string(),
        status: EndpointStatus::Pending,
    });

    let received = receiver.recv().await.unwrap();
    match received {
        DashboardEvent::EndpointRegistered {
            endpoint_id,
            endpoint_name,
            ip_address,
            status,
        } => {
            assert_eq!(endpoint_id, id);
            assert_eq!(endpoint_name, "test-machine");
            assert_eq!(ip_address, "10.0.0.1");
            assert_eq!(status, EndpointStatus::Pending);
        }
        _ => panic!("Expected NodeRegistered"),
    }
}

#[tokio::test]
async fn test_endpoint_status_changed_broadcast() {
    let bus = DashboardEventBus::new();
    let mut receiver = bus.subscribe();

    let id = Uuid::new_v4();
    bus.publish(DashboardEvent::EndpointStatusChanged {
        endpoint_id: id,
        old_status: EndpointStatus::Pending,
        new_status: EndpointStatus::Online,
    });

    let received = receiver.recv().await.unwrap();
    match received {
        DashboardEvent::EndpointStatusChanged {
            old_status,
            new_status,
            ..
        } => {
            assert_eq!(old_status, EndpointStatus::Pending);
            assert_eq!(new_status, EndpointStatus::Online);
        }
        _ => panic!("Expected EndpointStatusChanged"),
    }
}

#[tokio::test]
async fn test_metrics_updated_broadcast() {
    let bus = DashboardEventBus::new();
    let mut receiver = bus.subscribe();

    bus.publish(DashboardEvent::MetricsUpdated {
        endpoint_id: Uuid::new_v4(),
        cpu_usage: Some(42.0),
        memory_usage: Some(55.5),
        gpu_usage: None,
    });

    let received = receiver.recv().await.unwrap();
    match received {
        DashboardEvent::MetricsUpdated {
            cpu_usage,
            memory_usage,
            gpu_usage,
            ..
        } => {
            assert_eq!(cpu_usage, Some(42.0));
            assert_eq!(memory_usage, Some(55.5));
            assert!(gpu_usage.is_none());
        }
        _ => panic!("Expected MetricsUpdated"),
    }
}

// --- TpsUpdated edge cases ---

#[test]
fn test_tps_updated_zero_values_serialization() {
    let event = DashboardEvent::TpsUpdated {
        endpoint_id: Uuid::nil(),
        model_id: "".to_string(),
        tps: 0.0,
        output_tokens: 0,
        duration_ms: 0,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "TpsUpdated");
    let data = &json["data"];
    assert_eq!(data["tps"], 0.0);
    assert_eq!(data["output_tokens"], 0);
    assert_eq!(data["duration_ms"], 0);
}

#[test]
fn test_tps_updated_large_values_serialization() {
    let event = DashboardEvent::TpsUpdated {
        endpoint_id: Uuid::new_v4(),
        model_id: "large-model".to_string(),
        tps: 99999.99,
        output_tokens: u32::MAX,
        duration_ms: u64::MAX,
    };
    let json = serde_json::to_value(&event).unwrap();
    let data = &json["data"];
    assert_eq!(data["output_tokens"], u32::MAX);
}

// --- Event channel capacity ---

#[test]
fn test_event_channel_capacity_constant() {
    assert_eq!(EVENT_CHANNEL_CAPACITY, 1024);
}
