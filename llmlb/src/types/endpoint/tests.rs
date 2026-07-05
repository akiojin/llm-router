use super::*;

#[test]
fn test_endpoint_status_serialization() {
    assert_eq!(
        serde_json::to_string(&EndpointStatus::Pending).unwrap(),
        "\"pending\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointStatus::Online).unwrap(),
        "\"online\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointStatus::Offline).unwrap(),
        "\"offline\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointStatus::Error).unwrap(),
        "\"error\""
    );
}

#[test]
fn test_endpoint_status_from_str() {
    assert_eq!(
        "pending".parse::<EndpointStatus>().unwrap(),
        EndpointStatus::Pending
    );
    assert_eq!(
        "online".parse::<EndpointStatus>().unwrap(),
        EndpointStatus::Online
    );
    assert_eq!(
        "offline".parse::<EndpointStatus>().unwrap(),
        EndpointStatus::Offline
    );
    assert_eq!(
        "error".parse::<EndpointStatus>().unwrap(),
        EndpointStatus::Error
    );
    assert_eq!(
        "unknown".parse::<EndpointStatus>().unwrap(),
        EndpointStatus::Pending
    );
}

#[test]
fn test_endpoint_new() {
    let endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    assert_eq!(endpoint.name, "Test");
    assert_eq!(endpoint.base_url, "http://localhost:8080");
    assert_eq!(endpoint.status, EndpointStatus::Pending);
    assert_eq!(endpoint.endpoint_type, EndpointType::Xllm);
    assert_eq!(endpoint.health_check_interval_secs, 30);
    assert_eq!(endpoint.inference_timeout_secs, 600);
    assert_eq!(endpoint.error_count, 0);
    // デフォルトでChatCompletion機能を持つ
    assert!(endpoint.has_capability(EndpointCapability::ChatCompletion));
    assert!(!endpoint.has_capability(EndpointCapability::ImageGeneration));
}

#[test]
fn test_endpoint_type_recommended_inference_timeout() {
    let test_cases = [
        (EndpointType::Xllm, 600),
        (EndpointType::Ollama, 600),
        (EndpointType::LmStudio, 600),
        (EndpointType::Vllm, 120),
        (EndpointType::Llamacpp, 120),
        (EndpointType::OpenaiCompatible, 120),
    ];

    for (endpoint_type, expected_timeout) in test_cases {
        assert_eq!(
            endpoint_type.recommended_inference_timeout_secs(),
            expected_timeout
        );
    }
}

#[test]
fn test_endpoint_capability_serialization() {
    assert_eq!(
        serde_json::to_string(&EndpointCapability::ChatCompletion).unwrap(),
        "\"chat_completion\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointCapability::ImageGeneration).unwrap(),
        "\"image_generation\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointCapability::AudioTranscription).unwrap(),
        "\"audio_transcription\""
    );
}

#[test]
fn test_supported_api_serialization() {
    // SupportedAPI列挙型のシリアライズテスト (SPEC-0f1de549)
    assert_eq!(
        serde_json::to_string(&SupportedAPI::ChatCompletions).unwrap(),
        "\"chat_completions\""
    );
    assert_eq!(
        serde_json::to_string(&SupportedAPI::Responses).unwrap(),
        "\"responses\""
    );
    assert_eq!(
        serde_json::to_string(&SupportedAPI::Embeddings).unwrap(),
        "\"embeddings\""
    );
    assert_eq!(
        serde_json::to_string(&SupportedAPI::ImageInput).unwrap(),
        "\"image_input\""
    );
    assert_eq!(
        serde_json::to_string(&SupportedAPI::ImageGeneration).unwrap(),
        "\"image_generation\""
    );
}

#[test]
fn test_supported_api_as_str() {
    assert_eq!(SupportedAPI::ChatCompletions.as_str(), "chat_completions");
    assert_eq!(SupportedAPI::Responses.as_str(), "responses");
    assert_eq!(SupportedAPI::Embeddings.as_str(), "embeddings");
    assert_eq!(SupportedAPI::ImageInput.as_str(), "image_input");
    assert_eq!(SupportedAPI::ImageGeneration.as_str(), "image_generation");
}

#[test]
fn test_supported_api_from_api_str_accepts_vision_aliases() {
    assert_eq!(
        SupportedAPI::from_api_str("vision"),
        Some(SupportedAPI::ImageInput)
    );
    assert_eq!(
        SupportedAPI::from_api_str("image-input"),
        Some(SupportedAPI::ImageInput)
    );
    assert_eq!(
        SupportedAPI::from_api_str("image_generation"),
        Some(SupportedAPI::ImageGeneration)
    );
}

#[test]
fn test_endpoint_model_default_supported_apis() {
    // EndpointModelのデフォルトサポートAPI (SPEC-0f1de549)
    let json = r#"{"endpoint_id":"00000000-0000-0000-0000-000000000000","model_id":"test"}"#;
    let model: EndpointModel = serde_json::from_str(json).unwrap();
    assert_eq!(model.supported_apis.len(), 1);
    assert_eq!(model.supported_apis[0], SupportedAPI::ChatCompletions);
}

#[test]
fn test_endpoint_api_key_not_serialized() {
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    endpoint.api_key = Some("secret".to_string());

    let json = serde_json::to_string(&endpoint).unwrap();
    assert!(!json.contains("secret"));
    assert!(!json.contains("api_key"));
}

// SPEC-f8e3a1b7: レイテンシ計算テスト (T021)

#[test]
fn test_update_inference_latency_initial() {
    // 初回更新: None → Some(value)
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    assert!(endpoint.inference_latency_ms.is_none());

    endpoint.update_inference_latency(100.0);
    assert_eq!(endpoint.inference_latency_ms, Some(100.0));
}

#[test]
fn test_update_inference_latency_ema() {
    // EMA計算: α=0.2
    // new_ema = α * new + (1 - α) * old
    // new_ema = 0.2 * 200 + 0.8 * 100 = 40 + 80 = 120
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    endpoint.inference_latency_ms = Some(100.0);

    endpoint.update_inference_latency(200.0);
    assert_eq!(endpoint.inference_latency_ms, Some(120.0));

    // 続けて更新
    // new_ema = 0.2 * 100 + 0.8 * 120 = 20 + 96 = 116
    endpoint.update_inference_latency(100.0);
    assert_eq!(endpoint.inference_latency_ms, Some(116.0));
}

#[test]
fn test_reset_inference_latency() {
    // オフライン時にINFINITYにリセット
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    endpoint.inference_latency_ms = Some(100.0);

    endpoint.reset_inference_latency();
    assert_eq!(endpoint.inference_latency_ms, Some(f64::INFINITY));
}

#[test]
fn test_get_inference_latency_for_sort() {
    // None → INFINITY
    let endpoint1 = Endpoint::new(
        "Test1".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    assert_eq!(endpoint1.get_inference_latency_for_sort(), f64::INFINITY);

    // Some(value) → value
    let mut endpoint2 = Endpoint::new(
        "Test2".to_string(),
        "http://localhost:8081".to_string(),
        EndpointType::Xllm,
    );
    endpoint2.inference_latency_ms = Some(50.0);
    assert_eq!(endpoint2.get_inference_latency_for_sort(), 50.0);

    // Some(INFINITY) → INFINITY
    let mut endpoint3 = Endpoint::new(
        "Test3".to_string(),
        "http://localhost:8082".to_string(),
        EndpointType::Xllm,
    );
    endpoint3.reset_inference_latency();
    assert_eq!(endpoint3.get_inference_latency_for_sort(), f64::INFINITY);
}

#[test]
fn test_update_inference_latency_from_infinity() {
    // INFINITY状態からの復帰: 新しい値がそのまま設定される
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    endpoint.reset_inference_latency();
    assert_eq!(endpoint.inference_latency_ms, Some(f64::INFINITY));

    endpoint.update_inference_latency(100.0);
    assert_eq!(endpoint.inference_latency_ms, Some(100.0));
}

#[test]
fn test_device_type_serialization() {
    assert_eq!(serde_json::to_string(&DeviceType::Cpu).unwrap(), "\"cpu\"");
    assert_eq!(serde_json::to_string(&DeviceType::Gpu).unwrap(), "\"gpu\"");
}

#[test]
fn test_device_info_default() {
    let info = DeviceInfo::default();
    assert_eq!(info.device_type, DeviceType::Cpu);
    assert!(info.gpu_devices.is_empty());
}

#[test]
fn test_device_info_serialization() {
    let info = DeviceInfo {
        device_type: DeviceType::Gpu,
        gpu_devices: vec![GpuDevice {
            name: "NVIDIA RTX 4090".to_string(),
            total_memory_bytes: 24_000_000_000,
            used_memory_bytes: 8_000_000_000,
        }],
    };

    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"device_type\":\"gpu\""));
    assert!(json.contains("NVIDIA RTX 4090"));

    let deserialized: DeviceInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.device_type, DeviceType::Gpu);
    assert_eq!(deserialized.gpu_devices.len(), 1);
}

// SPEC-e8e9326e: エンドポイントタイプ自動判別機能テスト

#[test]
fn test_endpoint_type_serialization() {
    assert_eq!(
        serde_json::to_string(&EndpointType::Xllm).unwrap(),
        "\"xllm\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointType::Ollama).unwrap(),
        "\"ollama\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointType::Vllm).unwrap(),
        "\"vllm\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointType::LmStudio).unwrap(),
        "\"lm_studio\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointType::OpenaiCompatible).unwrap(),
        "\"openai_compatible\""
    );
}

#[test]
fn test_endpoint_type_deserialization_lm_studio() {
    let deserialized: EndpointType = serde_json::from_str("\"lm_studio\"").unwrap();
    assert_eq!(deserialized, EndpointType::LmStudio);
}

#[test]
fn test_endpoint_type_from_str() {
    assert_eq!("xllm".parse::<EndpointType>().unwrap(), EndpointType::Xllm);
    assert_eq!(
        "ollama".parse::<EndpointType>().unwrap(),
        EndpointType::Ollama
    );
    assert_eq!("vllm".parse::<EndpointType>().unwrap(), EndpointType::Vllm);
    assert_eq!(
        "lm_studio".parse::<EndpointType>().unwrap(),
        EndpointType::LmStudio
    );
    assert_eq!(
        "openai_compatible".parse::<EndpointType>().unwrap(),
        EndpointType::OpenaiCompatible
    );
    // 未知の値はエラーを返す
    assert!("unknown".parse::<EndpointType>().is_err());
    assert!("invalid".parse::<EndpointType>().is_err());
}

#[test]
fn test_endpoint_type_as_str() {
    assert_eq!(EndpointType::Xllm.as_str(), "xllm");
    assert_eq!(EndpointType::Ollama.as_str(), "ollama");
    assert_eq!(EndpointType::Vllm.as_str(), "vllm");
    assert_eq!(EndpointType::LmStudio.as_str(), "lm_studio");
    assert_eq!(EndpointType::OpenaiCompatible.as_str(), "openai_compatible");
}

#[test]
fn test_endpoint_type_supports_model_download() {
    // xLLM, Ollama, LM Studioがダウンロードをサポート
    assert!(EndpointType::Xllm.supports_model_download());
    assert!(EndpointType::Ollama.supports_model_download());
    assert!(EndpointType::LmStudio.supports_model_download());
    assert!(!EndpointType::Vllm.supports_model_download());
    assert!(!EndpointType::Llamacpp.supports_model_download());
    assert!(!EndpointType::OpenaiCompatible.supports_model_download());
}

// SPEC-4bb5b55f T001: TPS計測対象判定テスト
#[test]
fn test_endpoint_type_is_tps_trackable() {
    // xLLM, Ollama, vLLM, LmStudio, OpenaiCompatible はTPS計測対象
    assert!(EndpointType::Xllm.is_tps_trackable());
    assert!(EndpointType::Ollama.is_tps_trackable());
    assert!(EndpointType::Vllm.is_tps_trackable());
    assert!(EndpointType::LmStudio.is_tps_trackable());
    assert!(EndpointType::OpenaiCompatible.is_tps_trackable());
}

#[test]
fn test_endpoint_type_supports_model_metadata() {
    // xLLM、Ollama、LmStudioがメタデータ取得をサポート
    assert!(EndpointType::Xllm.supports_model_metadata());
    assert!(EndpointType::Ollama.supports_model_metadata());
    assert!(EndpointType::LmStudio.supports_model_metadata());
    assert!(!EndpointType::Vllm.supports_model_metadata());
    assert!(!EndpointType::Llamacpp.supports_model_metadata());
    assert!(!EndpointType::OpenaiCompatible.supports_model_metadata());
}

#[test]
fn test_download_status_serialization() {
    assert_eq!(
        serde_json::to_string(&DownloadStatus::Pending).unwrap(),
        "\"pending\""
    );
    assert_eq!(
        serde_json::to_string(&DownloadStatus::Downloading).unwrap(),
        "\"downloading\""
    );
    assert_eq!(
        serde_json::to_string(&DownloadStatus::Completed).unwrap(),
        "\"completed\""
    );
    assert_eq!(
        serde_json::to_string(&DownloadStatus::Failed).unwrap(),
        "\"failed\""
    );
    assert_eq!(
        serde_json::to_string(&DownloadStatus::Cancelled).unwrap(),
        "\"cancelled\""
    );
}

#[test]
fn test_download_status_from_str() {
    assert_eq!(
        "pending".parse::<DownloadStatus>().unwrap(),
        DownloadStatus::Pending
    );
    assert_eq!(
        "downloading".parse::<DownloadStatus>().unwrap(),
        DownloadStatus::Downloading
    );
    assert_eq!(
        "completed".parse::<DownloadStatus>().unwrap(),
        DownloadStatus::Completed
    );
    assert_eq!(
        "failed".parse::<DownloadStatus>().unwrap(),
        DownloadStatus::Failed
    );
    assert_eq!(
        "cancelled".parse::<DownloadStatus>().unwrap(),
        DownloadStatus::Cancelled
    );
    // 未知の値はPendingにフォールバック
    assert_eq!(
        "invalid".parse::<DownloadStatus>().unwrap(),
        DownloadStatus::Pending
    );
}

#[test]
fn test_model_download_task_new() {
    let endpoint_id = Uuid::new_v4();
    let task = ModelDownloadTask::new(endpoint_id, "llama-3.2-1b".to_string());

    assert_eq!(task.endpoint_id, endpoint_id);
    assert_eq!(task.model, "llama-3.2-1b");
    assert_eq!(task.status, DownloadStatus::Pending);
    assert_eq!(task.progress, 0.0);
    assert!(task.filename.is_none());
    assert!(task.speed_mbps.is_none());
    assert!(task.eta_seconds.is_none());
    assert!(task.error_message.is_none());
    assert!(task.completed_at.is_none());
}

#[test]
fn test_model_download_task_is_finished() {
    let endpoint_id = Uuid::new_v4();
    let mut task = ModelDownloadTask::new(endpoint_id, "test-model".to_string());

    // 初期状態はPending -> 未完了
    assert!(!task.is_finished());

    // Downloading -> 未完了
    task.status = DownloadStatus::Downloading;
    assert!(!task.is_finished());

    // Completed -> 完了
    task.status = DownloadStatus::Completed;
    assert!(task.is_finished());

    // Failed -> 完了
    task.status = DownloadStatus::Failed;
    assert!(task.is_finished());

    // Cancelled -> 完了
    task.status = DownloadStatus::Cancelled;
    assert!(task.is_finished());
}

#[test]
fn test_endpoint_new_has_endpoint_type() {
    let endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::OpenaiCompatible,
    );
    assert_eq!(endpoint.endpoint_type, EndpointType::OpenaiCompatible);
}

#[test]
fn test_endpoint_model_max_tokens() {
    let json = r#"{
            "endpoint_id": "00000000-0000-0000-0000-000000000000",
            "model_id": "test",
            "max_tokens": 4096
        }"#;
    let model: EndpointModel = serde_json::from_str(json).unwrap();
    assert_eq!(model.max_tokens, Some(4096));
}

// --- 追加テスト: EndpointStatus serde roundtrip ---

#[test]
fn test_endpoint_status_serde_roundtrip() {
    for status in [
        EndpointStatus::Pending,
        EndpointStatus::Online,
        EndpointStatus::Offline,
        EndpointStatus::Error,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: EndpointStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, status);
    }
}

#[test]
fn test_endpoint_status_display() {
    assert_eq!(EndpointStatus::Pending.to_string(), "pending");
    assert_eq!(EndpointStatus::Online.to_string(), "online");
    assert_eq!(EndpointStatus::Offline.to_string(), "offline");
    assert_eq!(EndpointStatus::Error.to_string(), "error");
}

#[test]
fn test_endpoint_status_default() {
    let status: EndpointStatus = Default::default();
    assert_eq!(status, EndpointStatus::Pending);
}

// --- 追加テスト: EndpointType serde roundtrip ---

#[test]
fn test_endpoint_type_serde_roundtrip() {
    for et in [
        EndpointType::Xllm,
        EndpointType::Ollama,
        EndpointType::Vllm,
        EndpointType::LmStudio,
        EndpointType::OpenaiCompatible,
    ] {
        let json = serde_json::to_string(&et).unwrap();
        let deserialized: EndpointType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, et);
    }
}

#[test]
fn test_endpoint_type_display() {
    assert_eq!(EndpointType::Xllm.to_string(), "xllm");
    assert_eq!(EndpointType::Ollama.to_string(), "ollama");
    assert_eq!(EndpointType::Vllm.to_string(), "vllm");
    assert_eq!(EndpointType::LmStudio.to_string(), "lm_studio");
    assert_eq!(
        EndpointType::OpenaiCompatible.to_string(),
        "openai_compatible"
    );
}

#[test]
fn test_parse_endpoint_type_error_display() {
    let err = ParseEndpointTypeError("foo".to_string());
    assert_eq!(err.to_string(), "unknown endpoint type: 'foo'");
}

// --- 追加テスト: DownloadStatus serde roundtrip ---

#[test]
fn test_download_status_serde_roundtrip() {
    for ds in [
        DownloadStatus::Pending,
        DownloadStatus::Downloading,
        DownloadStatus::Completed,
        DownloadStatus::Failed,
        DownloadStatus::Cancelled,
    ] {
        let json = serde_json::to_string(&ds).unwrap();
        let deserialized: DownloadStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ds);
    }
}

#[test]
fn test_download_status_display() {
    assert_eq!(DownloadStatus::Pending.to_string(), "pending");
    assert_eq!(DownloadStatus::Downloading.to_string(), "downloading");
    assert_eq!(DownloadStatus::Completed.to_string(), "completed");
    assert_eq!(DownloadStatus::Failed.to_string(), "failed");
    assert_eq!(DownloadStatus::Cancelled.to_string(), "cancelled");
}

#[test]
fn test_download_status_default() {
    let ds: DownloadStatus = Default::default();
    assert_eq!(ds, DownloadStatus::Pending);
}

// --- 追加テスト: EndpointCapability ---

#[test]
fn test_endpoint_capability_serde_roundtrip() {
    for cap in [
        EndpointCapability::ChatCompletion,
        EndpointCapability::Embeddings,
        EndpointCapability::ImageGeneration,
        EndpointCapability::AudioTranscription,
        EndpointCapability::AudioSpeech,
    ] {
        let json = serde_json::to_string(&cap).unwrap();
        let deserialized: EndpointCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, cap);
    }
}

#[test]
fn test_endpoint_capability_display() {
    assert_eq!(
        EndpointCapability::ChatCompletion.to_string(),
        "chat_completion"
    );
    assert_eq!(EndpointCapability::Embeddings.to_string(), "embeddings");
    assert_eq!(
        EndpointCapability::ImageGeneration.to_string(),
        "image_generation"
    );
    assert_eq!(
        EndpointCapability::AudioTranscription.to_string(),
        "audio_transcription"
    );
    assert_eq!(EndpointCapability::AudioSpeech.to_string(), "audio_speech");
}

// --- 追加テスト: SupportedAPI ---

#[test]
fn test_supported_api_serde_roundtrip() {
    for api in [
        SupportedAPI::ChatCompletions,
        SupportedAPI::Responses,
        SupportedAPI::Embeddings,
    ] {
        let json = serde_json::to_string(&api).unwrap();
        let deserialized: SupportedAPI = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, api);
    }
}

#[test]
fn test_supported_api_display() {
    assert_eq!(
        SupportedAPI::ChatCompletions.to_string(),
        "chat_completions"
    );
    assert_eq!(SupportedAPI::Responses.to_string(), "responses");
    assert_eq!(SupportedAPI::Embeddings.to_string(), "embeddings");
}

// --- 追加テスト: Endpoint構造体 ---

#[test]
fn test_endpoint_new_default_request_counts() {
    let ep = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Ollama,
    );
    assert_eq!(ep.total_requests, 0);
    assert_eq!(ep.successful_requests, 0);
    assert_eq!(ep.failed_requests, 0);
    assert!(ep.device_info.is_none());
    assert!(ep.inference_latency_ms.is_none());
    assert!(ep.active_requests.is_none());
}

#[test]
fn test_endpoint_serde_roundtrip() {
    let ep = Endpoint::new(
        "RoundTrip".to_string(),
        "http://10.0.0.1:8080".to_string(),
        EndpointType::Vllm,
    );
    let json = serde_json::to_string(&ep).unwrap();
    let deserialized: Endpoint = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "RoundTrip");
    assert_eq!(deserialized.endpoint_type, EndpointType::Vllm);
    assert_eq!(deserialized.status, EndpointStatus::Pending);
}

// --- 追加テスト: EndpointDailyStats ---

#[test]
fn test_endpoint_daily_stats_serde_roundtrip() {
    let stats = EndpointDailyStats {
        endpoint_id: Uuid::new_v4(),
        model_id: "llama3".to_string(),
        date: "2026-02-27".to_string(),
        total_requests: 100,
        successful_requests: 95,
        failed_requests: 5,
    };
    let json = serde_json::to_string(&stats).unwrap();
    let deserialized: EndpointDailyStats = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.model_id, "llama3");
    assert_eq!(deserialized.total_requests, 100);
    assert_eq!(deserialized.successful_requests, 95);
    assert_eq!(deserialized.failed_requests, 5);
}

// ========================================================================
// 追加テスト: DeviceType
// ========================================================================

#[test]
fn test_device_type_default_is_cpu() {
    let dt: DeviceType = Default::default();
    assert_eq!(dt, DeviceType::Cpu);
}

#[test]
fn test_device_type_serde_roundtrip() {
    for dt in [DeviceType::Cpu, DeviceType::Gpu] {
        let json = serde_json::to_string(&dt).unwrap();
        let deserialized: DeviceType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, dt);
    }
}

#[test]
fn test_device_type_deserialization() {
    let cpu: DeviceType = serde_json::from_str("\"cpu\"").unwrap();
    assert_eq!(cpu, DeviceType::Cpu);
    let gpu: DeviceType = serde_json::from_str("\"gpu\"").unwrap();
    assert_eq!(gpu, DeviceType::Gpu);
}

#[test]
fn test_device_type_invalid_deserialization_fails() {
    let result = serde_json::from_str::<DeviceType>("\"tpu\"");
    assert!(result.is_err());
}

// ========================================================================
// 追加テスト: GpuDevice
// ========================================================================

#[test]
fn test_gpu_device_serde_roundtrip() {
    let device = GpuDevice {
        name: "Apple M2 Ultra".to_string(),
        total_memory_bytes: 192_000_000_000,
        used_memory_bytes: 64_000_000_000,
    };
    let json = serde_json::to_string(&device).unwrap();
    let deserialized: GpuDevice = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "Apple M2 Ultra");
    assert_eq!(deserialized.total_memory_bytes, 192_000_000_000);
    assert_eq!(deserialized.used_memory_bytes, 64_000_000_000);
}

#[test]
fn test_gpu_device_used_memory_default_zero() {
    // used_memory_bytes has serde(default), so it should default to 0
    let json = r#"{"name":"RTX 4090","total_memory_bytes":24000000000}"#;
    let device: GpuDevice = serde_json::from_str(json).unwrap();
    assert_eq!(device.used_memory_bytes, 0);
}

#[test]
fn test_gpu_device_zero_memory() {
    let device = GpuDevice {
        name: "".to_string(),
        total_memory_bytes: 0,
        used_memory_bytes: 0,
    };
    let json = serde_json::to_string(&device).unwrap();
    let deserialized: GpuDevice = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_memory_bytes, 0);
}

#[test]
fn test_gpu_device_max_memory_values() {
    let device = GpuDevice {
        name: "Huge GPU".to_string(),
        total_memory_bytes: u64::MAX,
        used_memory_bytes: u64::MAX,
    };
    let json = serde_json::to_string(&device).unwrap();
    let deserialized: GpuDevice = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_memory_bytes, u64::MAX);
    assert_eq!(deserialized.used_memory_bytes, u64::MAX);
}

// ========================================================================
// 追加テスト: DeviceInfo
// ========================================================================

#[test]
fn test_device_info_with_multiple_gpus() {
    let info = DeviceInfo {
        device_type: DeviceType::Gpu,
        gpu_devices: vec![
            GpuDevice {
                name: "GPU 0".to_string(),
                total_memory_bytes: 24_000_000_000,
                used_memory_bytes: 0,
            },
            GpuDevice {
                name: "GPU 1".to_string(),
                total_memory_bytes: 24_000_000_000,
                used_memory_bytes: 12_000_000_000,
            },
        ],
    };
    let json = serde_json::to_string(&info).unwrap();
    let deserialized: DeviceInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.gpu_devices.len(), 2);
    assert_eq!(deserialized.gpu_devices[0].name, "GPU 0");
    assert_eq!(deserialized.gpu_devices[1].name, "GPU 1");
}

#[test]
fn test_device_info_gpu_devices_default_empty() {
    // gpu_devices has serde(default), so it should default to empty vec
    let json = r#"{"device_type":"cpu"}"#;
    let info: DeviceInfo = serde_json::from_str(json).unwrap();
    assert!(info.gpu_devices.is_empty());
}

// ========================================================================
// 追加テスト: SupportedAPI
// ========================================================================

#[test]
fn test_supported_api_deserialization() {
    let chat: SupportedAPI = serde_json::from_str("\"chat_completions\"").unwrap();
    assert_eq!(chat, SupportedAPI::ChatCompletions);
    let resp: SupportedAPI = serde_json::from_str("\"responses\"").unwrap();
    assert_eq!(resp, SupportedAPI::Responses);
    let emb: SupportedAPI = serde_json::from_str("\"embeddings\"").unwrap();
    assert_eq!(emb, SupportedAPI::Embeddings);
}

#[test]
fn test_supported_api_invalid_deserialization_fails() {
    let result = serde_json::from_str::<SupportedAPI>("\"unknown_api\"");
    assert!(result.is_err());
}

// ========================================================================
// 追加テスト: EndpointStatus edge cases
// ========================================================================

#[test]
fn test_endpoint_status_from_str_empty_string() {
    // Empty string should fallback to Pending
    assert_eq!(
        "".parse::<EndpointStatus>().unwrap(),
        EndpointStatus::Pending
    );
}

#[test]
fn test_endpoint_status_from_str_case_sensitive() {
    // Uppercase should fallback to Pending (case-sensitive)
    assert_eq!(
        "ONLINE".parse::<EndpointStatus>().unwrap(),
        EndpointStatus::Pending
    );
    assert_eq!(
        "Online".parse::<EndpointStatus>().unwrap(),
        EndpointStatus::Pending
    );
}

#[test]
fn test_endpoint_status_as_str_matches_display() {
    for status in [
        EndpointStatus::Pending,
        EndpointStatus::Online,
        EndpointStatus::Offline,
        EndpointStatus::Error,
    ] {
        assert_eq!(status.as_str(), &status.to_string());
    }
}

// ========================================================================
// 追加テスト: EndpointType edge cases
// ========================================================================

#[test]
fn test_endpoint_type_from_str_empty_string_fails() {
    assert!("".parse::<EndpointType>().is_err());
}

#[test]
fn test_endpoint_type_from_str_case_sensitive() {
    assert!("XLLM".parse::<EndpointType>().is_err());
    assert!("Xllm".parse::<EndpointType>().is_err());
    assert!("OLLAMA".parse::<EndpointType>().is_err());
}

#[test]
fn test_endpoint_type_as_str_matches_display() {
    for et in [
        EndpointType::Xllm,
        EndpointType::Ollama,
        EndpointType::Vllm,
        EndpointType::LmStudio,
        EndpointType::OpenaiCompatible,
    ] {
        assert_eq!(et.as_str(), &et.to_string());
    }
}

#[test]
fn test_parse_endpoint_type_error_preserves_input() {
    let err = "some_random_value".parse::<EndpointType>().unwrap_err();
    assert_eq!(err.0, "some_random_value");
    assert!(err.to_string().contains("some_random_value"));
}

#[test]
fn test_parse_endpoint_type_error_is_error_trait() {
    let err = ParseEndpointTypeError("test".to_string());
    // Verify it implements std::error::Error via Display
    let _display = format!("{}", err);
    let _debug = format!("{:?}", err);
}

// ========================================================================
// 追加テスト: DownloadStatus edge cases
// ========================================================================

#[test]
fn test_download_status_from_str_empty_string() {
    assert_eq!(
        "".parse::<DownloadStatus>().unwrap(),
        DownloadStatus::Pending
    );
}

#[test]
fn test_download_status_from_str_case_sensitive() {
    assert_eq!(
        "COMPLETED".parse::<DownloadStatus>().unwrap(),
        DownloadStatus::Pending
    );
}

#[test]
fn test_download_status_as_str_matches_display() {
    for ds in [
        DownloadStatus::Pending,
        DownloadStatus::Downloading,
        DownloadStatus::Completed,
        DownloadStatus::Failed,
        DownloadStatus::Cancelled,
    ] {
        assert_eq!(ds.as_str(), &ds.to_string());
    }
}

// ========================================================================
// 追加テスト: EndpointCapability edge cases
// ========================================================================

#[test]
fn test_endpoint_capability_as_str() {
    assert_eq!(
        EndpointCapability::ChatCompletion.as_str(),
        "chat_completion"
    );
    assert_eq!(EndpointCapability::Embeddings.as_str(), "embeddings");
    assert_eq!(
        EndpointCapability::ImageGeneration.as_str(),
        "image_generation"
    );
    assert_eq!(
        EndpointCapability::AudioTranscription.as_str(),
        "audio_transcription"
    );
    assert_eq!(EndpointCapability::AudioSpeech.as_str(), "audio_speech");
}

#[test]
fn test_endpoint_capability_as_str_matches_display() {
    for cap in [
        EndpointCapability::ChatCompletion,
        EndpointCapability::Embeddings,
        EndpointCapability::ImageGeneration,
        EndpointCapability::AudioTranscription,
        EndpointCapability::AudioSpeech,
    ] {
        assert_eq!(cap.as_str(), &cap.to_string());
    }
}

#[test]
fn test_endpoint_capability_invalid_deserialization_fails() {
    let result = serde_json::from_str::<EndpointCapability>("\"video_generation\"");
    assert!(result.is_err());
}

// ========================================================================
// 追加テスト: Endpoint struct methods
// ========================================================================

#[test]
fn test_endpoint_has_capability_multiple() {
    let mut ep = Endpoint::new(
        "Multi".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    ep.capabilities = vec![
        EndpointCapability::ChatCompletion,
        EndpointCapability::Embeddings,
        EndpointCapability::AudioSpeech,
    ];
    assert!(ep.has_capability(EndpointCapability::ChatCompletion));
    assert!(ep.has_capability(EndpointCapability::Embeddings));
    assert!(ep.has_capability(EndpointCapability::AudioSpeech));
    assert!(!ep.has_capability(EndpointCapability::ImageGeneration));
    assert!(!ep.has_capability(EndpointCapability::AudioTranscription));
}

#[test]
fn test_endpoint_has_capability_empty() {
    let mut ep = Endpoint::new(
        "Empty".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    ep.capabilities = vec![];
    assert!(!ep.has_capability(EndpointCapability::ChatCompletion));
}

#[test]
fn test_endpoint_update_inference_latency_multiple_updates() {
    // Verify EMA converges toward repeated value
    let mut ep = Endpoint::new(
        "EMA".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    // Initial: 100.0
    ep.update_inference_latency(100.0);
    assert_eq!(ep.inference_latency_ms, Some(100.0));

    // Repeated updates with same value should stay at that value
    for _ in 0..20 {
        ep.update_inference_latency(100.0);
    }
    let lat = ep.inference_latency_ms.unwrap();
    assert!((lat - 100.0).abs() < 0.001);
}

#[test]
fn test_endpoint_update_inference_latency_zero() {
    let mut ep = Endpoint::new(
        "Zero".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    ep.update_inference_latency(0.0);
    assert_eq!(ep.inference_latency_ms, Some(0.0));
}

#[test]
fn test_endpoint_update_inference_latency_from_nan() {
    // NaN is not finite, so it should reset to the new value
    let mut ep = Endpoint::new(
        "NaN".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    ep.inference_latency_ms = Some(f64::NAN);
    ep.update_inference_latency(50.0);
    assert_eq!(ep.inference_latency_ms, Some(50.0));
}

#[test]
fn test_endpoint_get_inference_latency_for_sort_ordering() {
    // Verify that endpoints can be sorted by latency
    let mut eps: Vec<Endpoint> = (0..3)
        .map(|i| {
            Endpoint::new(
                format!("EP{}", i),
                format!("http://localhost:{}", 8080 + i),
                EndpointType::Xllm,
            )
        })
        .collect();
    eps[0].inference_latency_ms = Some(50.0);
    eps[1].inference_latency_ms = Some(100.0);
    eps[2].inference_latency_ms = None; // INFINITY

    eps.sort_by(|a, b| {
        a.get_inference_latency_for_sort()
            .partial_cmp(&b.get_inference_latency_for_sort())
            .unwrap()
    });
    assert_eq!(eps[0].name, "EP0");
    assert_eq!(eps[1].name, "EP1");
    assert_eq!(eps[2].name, "EP2");
}

// ========================================================================
// 追加テスト: Endpoint serialization edge cases
// ========================================================================

#[test]
fn test_endpoint_serialization_skips_api_key_when_none() {
    let ep = Endpoint::new(
        "NoKey".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    assert!(ep.api_key.is_none());
    let json = serde_json::to_string(&ep).unwrap();
    assert!(!json.contains("api_key"));
}

#[test]
fn test_endpoint_deserialization_with_optional_fields() {
    // Minimal JSON (relying on defaults)
    let json = r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "name": "Minimal",
            "base_url": "http://localhost:8080",
            "status": "online",
            "endpoint_type": "vllm",
            "health_check_interval_secs": 30,
            "inference_timeout_secs": 120,
            "error_count": 0,
            "registered_at": "2026-01-01T00:00:00Z",
            "total_requests": 0,
            "successful_requests": 0,
            "failed_requests": 0
        }"#;
    let ep: Endpoint = serde_json::from_str(json).unwrap();
    assert_eq!(ep.name, "Minimal");
    assert_eq!(ep.status, EndpointStatus::Online);
    assert_eq!(ep.endpoint_type, EndpointType::Vllm);
    assert!(ep.capabilities.is_empty()); // serde(default) → empty vec
    assert!(ep.device_info.is_none());
    assert!(ep.latency_ms.is_none());
    assert!(ep.last_seen.is_none());
    assert!(ep.last_error.is_none());
    assert!(ep.notes.is_none());
}

// ========================================================================
// 追加テスト: EndpointModel
// ========================================================================

#[test]
fn test_endpoint_model_serde_roundtrip() {
    let model = EndpointModel {
        endpoint_id: Uuid::nil(),
        model_id: "gpt-4".to_string(),
        capabilities: Some(vec!["chat".to_string()]),
        max_tokens: Some(8192),
        last_checked: None,
        supported_apis: vec![SupportedAPI::ChatCompletions, SupportedAPI::Responses],
        canonical_name: None,
    };
    let json = serde_json::to_string(&model).unwrap();
    let deserialized: EndpointModel = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.model_id, "gpt-4");
    assert_eq!(deserialized.max_tokens, Some(8192));
    assert_eq!(deserialized.supported_apis.len(), 2);
}

#[test]
fn test_endpoint_model_no_capabilities() {
    let json = r#"{
            "endpoint_id": "00000000-0000-0000-0000-000000000000",
            "model_id": "test",
            "capabilities": null,
            "max_tokens": null
        }"#;
    let model: EndpointModel = serde_json::from_str(json).unwrap();
    assert!(model.capabilities.is_none());
    assert!(model.max_tokens.is_none());
    // default_supported_apis should give ChatCompletions
    assert_eq!(model.supported_apis, vec![SupportedAPI::ChatCompletions]);
}

#[test]
fn test_endpoint_model_empty_model_id() {
    let json = r#"{
            "endpoint_id": "00000000-0000-0000-0000-000000000000",
            "model_id": ""
        }"#;
    let model: EndpointModel = serde_json::from_str(json).unwrap();
    assert_eq!(model.model_id, "");
}

// ========================================================================
// 追加テスト: ModelDownloadTask
// ========================================================================

#[test]
fn test_model_download_task_serde_roundtrip() {
    let task = ModelDownloadTask::new(Uuid::nil(), "test-model".to_string());
    let json = serde_json::to_string(&task).unwrap();
    let deserialized: ModelDownloadTask = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.model, "test-model");
    assert_eq!(deserialized.status, DownloadStatus::Pending);
    assert_eq!(deserialized.progress, 0.0);
}

#[test]
fn test_model_download_task_with_all_fields() {
    let mut task = ModelDownloadTask::new(Uuid::nil(), "llama3".to_string());
    task.filename = Some("model-q4_k_m.gguf".to_string());
    task.status = DownloadStatus::Downloading;
    task.progress = 0.75;
    task.speed_mbps = Some(50.5);
    task.eta_seconds = Some(120);
    task.error_message = None;

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: ModelDownloadTask = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.filename, Some("model-q4_k_m.gguf".to_string()));
    assert_eq!(deserialized.progress, 0.75);
    assert_eq!(deserialized.speed_mbps, Some(50.5));
    assert_eq!(deserialized.eta_seconds, Some(120));
}

#[test]
fn test_model_download_task_is_finished_pending_and_downloading() {
    let mut task = ModelDownloadTask::new(Uuid::nil(), "m".to_string());
    assert!(!task.is_finished()); // Pending

    task.status = DownloadStatus::Downloading;
    assert!(!task.is_finished());
}

#[test]
fn test_model_download_task_empty_model_name() {
    let task = ModelDownloadTask::new(Uuid::nil(), "".to_string());
    assert_eq!(task.model, "");
    assert!(!task.is_finished());
}

// ========================================================================
// 追加テスト: EndpointHealthCheck
// ========================================================================

#[test]
fn test_endpoint_health_check_serde_roundtrip() {
    let hc = EndpointHealthCheck {
        id: 1,
        endpoint_id: Uuid::nil(),
        checked_at: Utc::now(),
        success: true,
        latency_ms: Some(25),
        error_message: None,
        status_before: EndpointStatus::Pending,
        status_after: EndpointStatus::Online,
    };
    let json = serde_json::to_string(&hc).unwrap();
    let deserialized: EndpointHealthCheck = serde_json::from_str(&json).unwrap();
    assert!(deserialized.success);
    assert_eq!(deserialized.latency_ms, Some(25));
    assert!(deserialized.error_message.is_none());
    assert_eq!(deserialized.status_before, EndpointStatus::Pending);
    assert_eq!(deserialized.status_after, EndpointStatus::Online);
}

#[test]
fn test_endpoint_health_check_failure() {
    let hc = EndpointHealthCheck {
        id: 2,
        endpoint_id: Uuid::nil(),
        checked_at: Utc::now(),
        success: false,
        latency_ms: None,
        error_message: Some("Connection refused".to_string()),
        status_before: EndpointStatus::Online,
        status_after: EndpointStatus::Error,
    };
    let json = serde_json::to_string(&hc).unwrap();
    let deserialized: EndpointHealthCheck = serde_json::from_str(&json).unwrap();
    assert!(!deserialized.success);
    assert!(deserialized.latency_ms.is_none());
    assert_eq!(
        deserialized.error_message,
        Some("Connection refused".to_string())
    );
}

// ========================================================================
// 追加テスト: EndpointDailyStats edge cases
// ========================================================================

#[test]
fn test_endpoint_daily_stats_zero_requests() {
    let stats = EndpointDailyStats {
        endpoint_id: Uuid::nil(),
        model_id: "empty".to_string(),
        date: "2026-01-01".to_string(),
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
    };
    let json = serde_json::to_string(&stats).unwrap();
    let deserialized: EndpointDailyStats = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_requests, 0);
}

#[test]
fn test_endpoint_daily_stats_large_values() {
    let stats = EndpointDailyStats {
        endpoint_id: Uuid::nil(),
        model_id: "busy-model".to_string(),
        date: "2026-12-31".to_string(),
        total_requests: i64::MAX,
        successful_requests: i64::MAX - 1,
        failed_requests: 1,
    };
    let json = serde_json::to_string(&stats).unwrap();
    let deserialized: EndpointDailyStats = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_requests, i64::MAX);
}

#[test]
fn test_endpoint_daily_stats_empty_model_id() {
    let stats = EndpointDailyStats {
        endpoint_id: Uuid::nil(),
        model_id: "".to_string(),
        date: "2026-01-01".to_string(),
        total_requests: 1,
        successful_requests: 1,
        failed_requests: 0,
    };
    let json = serde_json::to_string(&stats).unwrap();
    let deserialized: EndpointDailyStats = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.model_id, "");
}
