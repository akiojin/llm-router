use super::*;

#[test]
fn test_resolve_canonical_by_ollama_name() {
    let result = resolve_canonical("gpt-oss:20b", &EndpointType::Ollama);
    assert_eq!(result, Some("openai/gpt-oss-20b"));
}

#[test]
fn test_resolve_canonical_by_lm_studio_name() {
    let result = resolve_canonical("openai/gpt-oss-20b", &EndpointType::LmStudio);
    assert_eq!(result, Some("openai/gpt-oss-20b"));
}

#[test]
fn test_resolve_canonical_by_canonical_name() {
    let result = resolve_canonical("openai/gpt-oss-20b", &EndpointType::Ollama);
    assert_eq!(result, Some("openai/gpt-oss-20b"));
}

#[test]
fn test_resolve_canonical_unknown() {
    let result = resolve_canonical("unknown-model", &EndpointType::Ollama);
    assert!(result.is_none());
}

#[test]
fn test_resolve_canonical_wrong_engine() {
    let result = resolve_canonical("gpt-oss:20b", &EndpointType::Vllm);
    assert!(result.is_none());
}

#[test]
fn test_resolve_engine_name_ollama() {
    let result = resolve_engine_name("openai/gpt-oss-20b", &EndpointType::Ollama);
    assert_eq!(result, Some("gpt-oss:20b"));
}

#[test]
fn test_resolve_engine_name_lm_studio() {
    let result = resolve_engine_name("openai/gpt-oss-20b", &EndpointType::LmStudio);
    assert_eq!(result, Some("openai/gpt-oss-20b"));
}

#[test]
fn test_resolve_engine_name_no_alias() {
    let result = resolve_engine_name("openai/gpt-oss-20b", &EndpointType::Vllm);
    assert!(result.is_none());
}

#[test]
fn test_resolve_engine_name_unknown_canonical() {
    let result = resolve_engine_name("unknown/model", &EndpointType::Ollama);
    assert!(result.is_none());
}

#[test]
fn test_resolve_engine_names_lm_studio_returns_all_aliases() {
    let result = resolve_engine_names("Qwen/Qwen3.5-35B-A3B", &EndpointType::LmStudio);
    assert_eq!(
        result,
        vec![
            "qwen3.5-35b-a3b",
            "qwen/qwen3.5-35b-a3b",
            "qwen/qwen3.5-35b-a3b:2"
        ]
    );
}

#[test]
fn test_resolve_engine_names_unknown_canonical_returns_empty() {
    let result = resolve_engine_names("unknown/model", &EndpointType::LmStudio);
    assert!(result.is_empty());
}

#[test]
fn test_supports_canonical_on_endpoint_true_when_alias_exists() {
    assert!(supports_canonical_on_endpoint(
        "openai/gpt-oss-20b",
        &EndpointType::Ollama
    ));
    assert!(supports_canonical_on_endpoint(
        "openai/gpt-oss-20b",
        &EndpointType::LmStudio
    ));
}

#[test]
fn test_supports_canonical_on_endpoint_false_when_alias_missing() {
    assert!(!supports_canonical_on_endpoint(
        "openai/gpt-oss-20b",
        &EndpointType::Vllm
    ));
    assert!(!supports_canonical_on_endpoint(
        "unknown/model",
        &EndpointType::Ollama
    ));
}

#[test]
fn test_find_mapping_by_canonical() {
    let mapping = find_mapping("openai/gpt-oss-20b");
    assert!(mapping.is_some());
    let m = mapping.unwrap();
    assert_eq!(m.canonical, "openai/gpt-oss-20b");
    assert!(!m.aliases.is_empty());
}

#[test]
fn test_find_mapping_by_alias() {
    let mapping = find_mapping("gpt-oss:20b");
    assert!(mapping.is_some());
    assert_eq!(mapping.unwrap().canonical, "openai/gpt-oss-20b");
}

#[test]
fn test_find_mapping_not_found() {
    let mapping = find_mapping("nonexistent-model");
    assert!(mapping.is_none());
}

#[test]
fn test_guess_hf_repo_lm_studio() {
    let result = guess_hf_repo(
        "lmstudio-community/gemma-3-1b-it-GGUF",
        &EndpointType::LmStudio,
    );
    assert_eq!(
        result,
        Some("lmstudio-community/gemma-3-1b-it-GGUF".to_string())
    );
}

#[test]
fn test_guess_hf_repo_lm_studio_no_slash() {
    let result = guess_hf_repo("gemma-3-1b", &EndpointType::LmStudio);
    assert!(result.is_none());
}

#[test]
fn test_guess_hf_repo_ollama() {
    let result = guess_hf_repo("gemma3:27b", &EndpointType::Ollama);
    assert!(result.is_none());
}

#[test]
fn test_resolve_canonical_any_by_canonical() {
    let result = resolve_canonical_any("openai/gpt-oss-20b");
    assert_eq!(result, Some("openai/gpt-oss-20b"));
}

#[test]
fn test_resolve_canonical_any_by_alias() {
    let result = resolve_canonical_any("gpt-oss:20b");
    assert_eq!(result, Some("openai/gpt-oss-20b"));
}

#[test]
fn test_resolve_canonical_any_unknown() {
    let result = resolve_canonical_any("unknown-model");
    assert!(result.is_none());
}

#[test]
fn test_builtin_mappings_not_empty() {
    assert!(!BUILTIN_MAPPINGS.is_empty());
}

#[test]
fn test_all_mappings_have_aliases() {
    for mapping in BUILTIN_MAPPINGS {
        assert!(
            !mapping.aliases.is_empty(),
            "Mapping for {} has no aliases",
            mapping.canonical
        );
    }
}

#[test]
fn test_qwen3_coder_mapping() {
    let result = resolve_canonical("qwen3-coder:30b", &EndpointType::Ollama);
    assert_eq!(result, Some("Qwen/Qwen3-Coder-30B-A3B-Instruct"));
}

#[test]
fn test_qwen3_coder_lm_studio_lowercase_mapping() {
    let result = resolve_canonical("qwen/qwen3-coder-30b", &EndpointType::LmStudio);
    assert_eq!(result, Some("Qwen/Qwen3-Coder-30B-A3B-Instruct"));
}

#[test]
fn test_qwen3_coder_latest_mapping() {
    let result = resolve_canonical("qwen3-coder:latest", &EndpointType::Ollama);
    assert_eq!(result, Some("Qwen/Qwen3-Coder-30B-A3B-Instruct"));
}

#[test]
fn test_qwen3_coder_next_mapping() {
    let result = resolve_canonical("qwen/qwen3-coder-next", &EndpointType::LmStudio);
    assert_eq!(result, Some("Qwen/Qwen3-Coder-Next"));
}

#[test]
fn test_qwen35_mapping() {
    let result = resolve_canonical("qwen3.5:latest", &EndpointType::Ollama);
    assert_eq!(result, Some("Qwen/Qwen3.5-35B-A3B"));
}

#[test]
fn test_glm47_mapping() {
    // canonical は zai-org に統一（HF 上の現行リポジトリ）
    let result = resolve_canonical("zai-org/glm-4.7-flash", &EndpointType::LmStudio);
    assert_eq!(result, Some("zai-org/glm-4.7-flash"));

    // 旧 THUDM 名は alias として canonical に解決される
    let legacy = resolve_canonical("THUDM/glm-4.7-flash", &EndpointType::LmStudio);
    assert_eq!(legacy, Some("zai-org/glm-4.7-flash"));
}

#[test]
fn test_nomic_embedding_mapping() {
    let result = resolve_canonical(
        "text-embedding-nomic-embed-text-v1.5",
        &EndpointType::LmStudio,
    );
    assert_eq!(result, Some("nomic-ai/nomic-embed-text-v1.5"));
}

#[test]
fn test_nemotron_super_unsloth_mapping() {
    let result = resolve_canonical(
        "unsloth/nvidia-nemotron-3-super-120b-a12b",
        &EndpointType::LmStudio,
    );
    assert_eq!(result, Some("nvidia/nemotron-3-super-120b-a12b"));
}

#[test]
fn test_gemma3_mapping() {
    let result = resolve_canonical("gemma3:27b", &EndpointType::Ollama);
    assert_eq!(result, Some("google/gemma-3-27b-it"));
}

#[test]
fn test_llama33_mapping() {
    let result = resolve_canonical("llama3.3:70b", &EndpointType::Ollama);
    assert_eq!(result, Some("meta-llama/Llama-3.3-70B-Instruct"));
}

#[test]
fn test_nvidia_nemotron_super_mapping() {
    let result = resolve_canonical("nemotron-3-super:120b-a12b", &EndpointType::Ollama);
    assert_eq!(result, Some("nvidia/nemotron-3-super-120b-a12b"));
}

#[test]
fn test_nvidia_nemotron_nano_mapping() {
    let ollama = resolve_canonical("nemotron-3-nano:30b", &EndpointType::Ollama);
    assert_eq!(ollama, Some("nvidia/Nemotron-3-Nano"));

    let result = resolve_canonical("nvidia/nemotron-3-nano", &EndpointType::LmStudio);
    assert_eq!(result, Some("nvidia/Nemotron-3-Nano"));
}

#[test]
fn test_nomic_embed_mapping() {
    let ollama = resolve_canonical("nomic-embed-text:latest", &EndpointType::Ollama);
    assert_eq!(ollama, Some("nomic-ai/nomic-embed-text-v1.5"));

    let result = resolve_canonical(
        "text-embedding-nomic-embed-text-v1.5",
        &EndpointType::LmStudio,
    );
    assert_eq!(result, Some("nomic-ai/nomic-embed-text-v1.5"));
}

#[test]
fn test_glm_flash_mapping() {
    let ollama = resolve_canonical("glm-4.7-flash:latest", &EndpointType::Ollama);
    assert_eq!(ollama, Some("zai-org/glm-4.7-flash"));

    let result = resolve_canonical("zai-org/glm-4.7-flash", &EndpointType::LmStudio);
    assert_eq!(result, Some("zai-org/glm-4.7-flash"));

    // legacy THUDM/... も alias 経由で canonical に解決される
    let legacy = resolve_canonical("THUDM/glm-4.7-flash", &EndpointType::LmStudio);
    assert_eq!(legacy, Some("zai-org/glm-4.7-flash"));
}

#[test]
fn test_qwen25_awq_mapping() {
    let ollama = resolve_canonical("qwen2.5:14b-instruct", &EndpointType::Ollama);
    assert_eq!(ollama, Some("Qwen/Qwen2.5-14B-Instruct-AWQ"));

    let result = resolve_canonical("Qwen/Qwen2.5-14B-Instruct-AWQ", &EndpointType::LmStudio);
    assert_eq!(result, Some("Qwen/Qwen2.5-14B-Instruct-AWQ"));
}

#[test]
fn test_qwen35_all_variants_resolve_to_same_canonical() {
    let ollama = resolve_canonical("qwen3.5:35b-a3b", &EndpointType::Ollama);
    let ollama_legacy = resolve_canonical("qwen3.5-35b-a3b", &EndpointType::Ollama);
    let lms_short = resolve_canonical("qwen3.5-35b-a3b", &EndpointType::LmStudio);
    let lms = resolve_canonical("qwen/qwen3.5-35b-a3b", &EndpointType::LmStudio);
    let lms_v2 = resolve_canonical("qwen/qwen3.5-35b-a3b:2", &EndpointType::LmStudio);
    assert_eq!(ollama, ollama_legacy);
    assert_eq!(ollama, lms_short);
    assert_eq!(lms_short, lms);
    assert_eq!(lms, lms_v2);
    assert_eq!(ollama, Some("Qwen/Qwen3.5-35B-A3B"));
}

#[test]
fn test_gemma4_ollama_resolves_to_canonical() {
    // `gemma4:latest` は撤廃済み（将来世代の登場で意味がねじれるため）。
    let removed = resolve_canonical("gemma4:latest", &EndpointType::Ollama);
    assert_eq!(removed, None);

    // 具体タグ `gemma4` は引き続き alias として解決される。
    let result = resolve_canonical("gemma4", &EndpointType::Ollama);
    assert_eq!(result, Some("google/gemma-4-26b-a4b"));
}

#[test]
fn test_gemma4_lm_studio_resolves_to_canonical() {
    let result = resolve_canonical("google/gemma-4-26b-a4b", &EndpointType::LmStudio);
    assert_eq!(result, Some("google/gemma-4-26b-a4b"));
}

#[test]
fn test_gemma4_engine_name_resolution() {
    // `:latest` 撤廃により Ollama 側の優先 alias は具体タグ `gemma4`
    let ollama = resolve_engine_name("google/gemma-4-26b-a4b", &EndpointType::Ollama);
    assert_eq!(ollama, Some("gemma4"));

    let lms = resolve_engine_name("google/gemma-4-26b-a4b", &EndpointType::LmStudio);
    assert_eq!(lms, Some("google/gemma-4-26b-a4b"));
}

#[test]
fn test_nemotron_nano_4b_alias_resolves() {
    let result = resolve_canonical("nvidia/nemotron-3-nano-4b", &EndpointType::LmStudio);
    assert_eq!(result, Some("nvidia/Nemotron-3-Nano"));
}

#[test]
fn test_recently_added_lm_studio_aliases_resolve() {
    let cases = [
        ("openai/gpt-oss-120b", "openai/gpt-oss-120b"),
        ("Qwen/Qwen3-Coder-30B-A3B-Instruct", "qwen/qwen3-coder-30b"),
        ("Qwen/Qwen3-30B", "qwen/qwen3-30b-a3b"),
        ("meta-llama/Llama-3.3-70B-Instruct", "meta/llama-3.3-70b"),
        ("google/gemma-3-27b-it", "google/gemma-3-27b"),
        (
            "nvidia/nemotron-3-super-120b-a12b",
            "nvidia-nemotron-3-super-120b-a12b",
        ),
    ];

    for (canonical, alias) in cases {
        let result = resolve_canonical(alias, &EndpointType::LmStudio);
        assert_eq!(result, Some(canonical), "failed for {}", alias);
    }
}

#[test]
fn test_build_canonical_maps_merges_aliases() {
    let models = vec![
        ("gpt-oss:20b", Some("openai/gpt-oss-20b")),
        ("openai/gpt-oss-20b", Some("openai/gpt-oss-20b")),
        ("qwen3.5:35b-a3b", Some("Qwen/Qwen3.5-35B-A3B")),
        ("qwen/qwen3.5-35b-a3b", Some("Qwen/Qwen3.5-35B-A3B")),
        ("unknown-model", None),
    ];
    let res = build_canonical_maps(models.into_iter());

    assert_eq!(res.canonical_to_aliases.len(), 2);
    assert!(res
        .canonical_to_aliases
        .get("openai/gpt-oss-20b")
        .unwrap()
        .contains("gpt-oss:20b"));
    assert!(res
        .canonical_to_aliases
        .get("Qwen/Qwen3.5-35B-A3B")
        .unwrap()
        .contains("qwen3.5:35b-a3b"));
    assert!(res
        .canonical_to_aliases
        .get("Qwen/Qwen3.5-35B-A3B")
        .unwrap()
        .contains("qwen/qwen3.5-35b-a3b"));
}

#[test]
fn test_canonical_for_returns_canonical_when_key_is_canonical() {
    let models = vec![
        ("gpt-oss:20b", Some("openai/gpt-oss-20b")),
        ("openai/gpt-oss-20b", Some("openai/gpt-oss-20b")),
    ];
    let res = build_canonical_maps(models.into_iter());

    assert_eq!(
        res.canonical_for("openai/gpt-oss-20b"),
        "openai/gpt-oss-20b"
    );
}

#[test]
fn test_canonical_for_returns_canonical_when_key_is_alias() {
    let models = vec![("gpt-oss:20b", Some("openai/gpt-oss-20b"))];
    let res = build_canonical_maps(models.into_iter());

    assert_eq!(res.canonical_for("gpt-oss:20b"), "openai/gpt-oss-20b");
}

#[test]
fn test_split_quantization_suffix_extracts_quantization_tag() {
    // GGUF 量子化サフィックス
    let (base, q) = split_quantization_suffix("ggml-org/gemma-4-E4B-it-GGUF:Q4_K_M");
    assert_eq!(base, "ggml-org/gemma-4-E4B-it-GGUF");
    assert_eq!(q, Some("Q4_K_M"));

    // Q5_K_M, Q8_0
    let (_, q5) = split_quantization_suffix("foo/bar:Q5_K_M");
    assert_eq!(q5, Some("Q5_K_M"));
    let (_, q8) = split_quantization_suffix("foo/bar:Q8_0");
    assert_eq!(q8, Some("Q8_0"));

    // 浮動小数点フォーマット
    let (_, f16) = split_quantization_suffix("foo/bar:F16");
    assert_eq!(f16, Some("F16"));
    let (_, bf16) = split_quantization_suffix("foo/bar:BF16");
    assert_eq!(bf16, Some("BF16"));

    // imatrix 量子化（IQ*）
    let (_, iq) = split_quantization_suffix("foo/bar:IQ4_XS");
    assert_eq!(iq, Some("IQ4_XS"));
}

#[test]
fn test_split_quantization_suffix_returns_none_for_non_quantization_tags() {
    // Ollama タグ（`:30b` `:latest` 等）は量子化ではない
    let (base, q) = split_quantization_suffix("qwen3-coder:30b");
    assert_eq!(base, "qwen3-coder:30b");
    assert_eq!(q, None);

    let (base2, q2) = split_quantization_suffix("gemma3:latest");
    assert_eq!(base2, "gemma3:latest");
    assert_eq!(q2, None);

    // コロンなし
    let (base3, q3) = split_quantization_suffix("openai/gpt-oss-20b");
    assert_eq!(base3, "openai/gpt-oss-20b");
    assert_eq!(q3, None);
}

#[test]
fn test_canonical_for_falls_back_to_quantization_stripped_lookup() {
    // base 名が canonical 表に登録されているとき、量子化付き ID も同じ canonical へ解決される
    let models = vec![("gguf-base", Some("vendor/base"))];
    let res = build_canonical_maps(models.into_iter());

    // suffix 付き ID は逆引きにないが、suffix 除去後にヒットする
    assert_eq!(res.canonical_for("gguf-base:Q4_K_M"), "vendor/base");
    assert_eq!(res.canonical_for("gguf-base:F16"), "vendor/base");

    // 量子化タグでない suffix は self-canonical fallback のまま
    assert_eq!(res.canonical_for("gguf-base:foo"), "gguf-base:foo");
}

#[test]
fn test_known_max_tokens_returns_value_for_known_canonical() {
    assert_eq!(known_max_tokens("openai/gpt-oss-20b"), Some(131_072));
    assert_eq!(
        known_max_tokens("Qwen/Qwen3-Coder-30B-A3B-Instruct"),
        Some(262_144)
    );
    assert_eq!(known_max_tokens("zai-org/glm-4.7-flash"), Some(131_072));
    assert_eq!(
        known_max_tokens("nomic-ai/nomic-embed-text-v1.5"),
        Some(8_192)
    );
}

#[test]
fn test_known_max_tokens_returns_none_for_unknown() {
    assert_eq!(known_max_tokens("ggml-org/gemma-4-E2B-it-GGUF"), None);
    assert_eq!(known_max_tokens(""), None);
}

#[test]
fn test_resolve_max_tokens_prefers_endpoint_reported() {
    // endpoint 申告がある場合はそれを採用（既知テーブルより優先）
    let resolved = resolve_max_tokens("openai/gpt-oss-20b", Some(65_536));
    assert_eq!(resolved, Some(65_536));
}

#[test]
fn test_resolve_max_tokens_falls_back_to_known_table() {
    // endpoint 申告が無い場合は known テーブルから取得
    let resolved = resolve_max_tokens("openai/gpt-oss-20b", None);
    assert_eq!(resolved, Some(131_072));
}

#[test]
fn test_resolve_max_tokens_returns_none_for_unknown() {
    // どちらにも無ければ None（レスポンスでは null）
    let resolved = resolve_max_tokens("totally-unknown-model", None);
    assert_eq!(resolved, None);
}

#[test]
fn test_canonical_for_returns_self_for_unknown() {
    // self-canonical fallback: mapping 未登録のモデルは id 自身を canonical とする
    // （`/v1/models` レスポンスで canonical_name: null を出さない方針）
    let models = vec![("gpt-oss:20b", Some("openai/gpt-oss-20b"))];
    let res = build_canonical_maps(models.into_iter());

    assert_eq!(res.canonical_for("unknown-model"), "unknown-model");
    assert!(!res.is_known("unknown-model"));
    assert!(res.is_known("openai/gpt-oss-20b"));
    assert!(res.is_known("gpt-oss:20b"));
}

#[test]
fn test_aliases_for_returns_sorted_aliases() {
    let models = vec![
        ("qwen3.5:35b-a3b", Some("Qwen/Qwen3.5-35B-A3B")),
        ("qwen/qwen3.5-35b-a3b", Some("Qwen/Qwen3.5-35B-A3B")),
    ];
    let res = build_canonical_maps(models.into_iter());
    let aliases = res.aliases_for("Qwen/Qwen3.5-35B-A3B");
    assert_eq!(aliases, vec!["qwen/qwen3.5-35b-a3b", "qwen3.5:35b-a3b"]);
}

#[test]
fn test_aliases_for_returns_empty_for_unknown() {
    let res = build_canonical_maps(std::iter::empty());
    assert!(res.aliases_for("anything").is_empty());
}

// --- US-029: identity 解析・一次配布元・ヒューリスティック grouping ---

#[test]
fn test_parse_identity_strips_owner_quant_format() {
    let id = parse_identity("ggml-org/gemma-4-E4B-it-GGUF:Q4_K_M");
    assert_eq!(id.family, "gemma-4");
    assert_eq!(id.size.as_deref(), Some("e4b"));
    assert!(id.instruct);
}

#[test]
fn test_parse_identity_base_vs_it_differs() {
    let base = parse_identity("google/gemma-4-26b-a4b");
    let it = parse_identity("google/gemma-4-26B-A4B-it");
    assert_eq!(base.family, "gemma-4");
    assert_eq!(base.size.as_deref(), Some("26b-a4b"));
    assert!(!base.instruct);
    assert!(it.instruct);
    // base と it は別 identity
    assert_ne!(base.group_key(), it.group_key());
}

#[test]
fn test_parse_identity_size_differs() {
    let e2b = parse_identity("ggml-org/gemma-4-E2B-it-GGUF");
    let e4b = parse_identity("ggml-org/gemma-4-E4B-it-GGUF");
    assert_eq!(e2b.size.as_deref(), Some("e2b"));
    assert_eq!(e4b.size.as_deref(), Some("e4b"));
    assert_ne!(e2b.group_key(), e4b.group_key());
}

#[test]
fn test_parse_identity_moe_size() {
    let id = parse_identity("nvidia/nemotron-3-super-120b-a12b");
    assert_eq!(id.family, "nemotron-3-super");
    assert_eq!(id.size.as_deref(), Some("120b-a12b"));
    assert!(!id.instruct);
}

#[test]
fn test_parse_identity_no_size_for_embeddings() {
    let id = parse_identity("nomic-ai/nomic-embed-text-v1.5");
    assert_eq!(id.size, None);
}

#[test]
fn test_first_party_org_for_family() {
    assert_eq!(first_party_org_for_family("gemma-4"), Some("google"));
    assert_eq!(first_party_org_for_family("qwen3-coder"), Some("Qwen"));
    assert_eq!(first_party_org_for_family("llama-3.3"), Some("meta-llama"));
    assert_eq!(first_party_org_for_family("gpt-oss"), Some("openai"));
    assert_eq!(first_party_org_for_family("totally-unknown"), None);
}

#[test]
fn test_heuristic_merges_redistributor_into_first_party() {
    // 同一 identity（gemma-4 / e4b / it）で google(一次配布元) と ggml-org(再配布)
    let models = vec![
        ("google/gemma-4-e4b-it", None),
        ("ggml-org/gemma-4-E4B-it-GGUF", None),
    ];
    let res = build_canonical_maps(models.into_iter());
    assert_eq!(
        res.canonical_for("ggml-org/gemma-4-E4B-it-GGUF"),
        "google/gemma-4-e4b-it"
    );
    assert_eq!(
        res.canonical_for("google/gemma-4-e4b-it"),
        "google/gemma-4-e4b-it"
    );
    assert!(res
        .aliases_for("google/gemma-4-e4b-it")
        .contains(&"ggml-org/gemma-4-E4B-it-GGUF".to_string()));
}

#[test]
fn test_heuristic_keeps_sizes_separate() {
    // サイズ違いはマージしない
    let models = vec![
        ("google/gemma-4-e2b-it", None),
        ("google/gemma-4-e4b-it", None),
    ];
    let res = build_canonical_maps(models.into_iter());
    assert_eq!(
        res.canonical_for("google/gemma-4-e2b-it"),
        "google/gemma-4-e2b-it"
    );
    assert_eq!(
        res.canonical_for("google/gemma-4-e4b-it"),
        "google/gemma-4-e4b-it"
    );
}

#[test]
fn test_heuristic_no_merge_without_first_party() {
    // 一次配布元 owner が不在ならマージしない（self-canonical）
    let models = vec![
        ("ggml-org/gemma-4-E4B-it-GGUF", None),
        ("bartowski/gemma-4-E4B-it-GGUF", None),
    ];
    let res = build_canonical_maps(models.into_iter());
    assert_eq!(
        res.canonical_for("ggml-org/gemma-4-E4B-it-GGUF"),
        "ggml-org/gemma-4-E4B-it-GGUF"
    );
    assert_eq!(
        res.canonical_for("bartowski/gemma-4-E4B-it-GGUF"),
        "bartowski/gemma-4-E4B-it-GGUF"
    );
}

#[test]
fn test_heuristic_does_not_break_explicit_canonical() {
    // 明示 canonical を持つモデルは Pass1 のまま
    let models = vec![
        ("gpt-oss:20b", Some("openai/gpt-oss-20b")),
        ("openai/gpt-oss-20b", Some("openai/gpt-oss-20b")),
    ];
    let res = build_canonical_maps(models.into_iter());
    assert_eq!(res.canonical_for("gpt-oss:20b"), "openai/gpt-oss-20b");
}

#[test]
fn test_heuristic_merges_redistributor_into_explicit_first_party() {
    // Codex review 回帰: BUILTIN 由来で explicit canonical を持つ一次配布元モデル
    // （google/gemma-4-26b-a4b）に、explicit canonical を持たない再配布 GGUF が
    // 同一 identity として集約されること（Pass 1 の first-party もアンカーになる）。
    let models = vec![
        ("gemma4", Some("google/gemma-4-26b-a4b")), // Ollama alias (Pass1)
        ("google/gemma-4-26b-a4b", Some("google/gemma-4-26b-a4b")), // self explicit (Pass1)
        ("ggml-org/gemma-4-26B-A4B-GGUF", None),    // redistributor (Pass2)
    ];
    let res = build_canonical_maps(models.into_iter());
    assert_eq!(
        res.canonical_for("ggml-org/gemma-4-26B-A4B-GGUF"),
        "google/gemma-4-26b-a4b"
    );
    let aliases = res.aliases_for("google/gemma-4-26b-a4b");
    assert!(aliases.contains(&"ggml-org/gemma-4-26B-A4B-GGUF".to_string()));
    assert!(aliases.contains(&"gemma4".to_string()));
}

#[test]
fn test_heuristic_no_anchor_when_first_party_is_redistributor_only() {
    // 一次配布元が explicit でも self でも存在しない場合はマージしない。
    let models = vec![
        ("gemma4", Some("google/gemma-4-26b-a4b")), // 別 identity (base/26b-a4b)
        ("bartowski/qwen3-99b-it-GGUF", None),      // 一次配布元(Qwen)未観測の re-dist
    ];
    let res = build_canonical_maps(models.into_iter());
    assert_eq!(
        res.canonical_for("bartowski/qwen3-99b-it-GGUF"),
        "bartowski/qwen3-99b-it-GGUF"
    );
}
