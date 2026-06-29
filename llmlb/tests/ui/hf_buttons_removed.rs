use std::fs;
use std::path::{Path, PathBuf};

// Contract tests for HF button removal (FR-028)
// Models section should not have Hugging Face related buttons

// Read the index.html file content for testing
fn get_file_content(relative_path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path: PathBuf = root.join(relative_path);
    fs::read_to_string(path).expect("Failed to read static dashboard asset")
}

fn get_index_html() -> String {
    get_file_content("src/web/static/index.html")
}

fn contains_japanese(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{3040}'..='\u{30FF}').contains(&ch) // Hiragana + Katakana
            || ('\u{3400}'..='\u{4DBF}').contains(&ch) // CJK Extension A
            || ('\u{4E00}'..='\u{9FFF}').contains(&ch) // CJK Unified Ideographs
            || ('\u{2E80}'..='\u{2FDF}').contains(&ch) // CJK Radicals + punctuation
            || ('\u{FF66}'..='\u{FF9F}').contains(&ch) // Halfwidth Katakana
    })
}

fn assert_no_japanese(text: &str, location: &str) {
    assert!(
        !contains_japanese(text),
        "{location} contains Japanese text"
    )
}

#[test]
fn models_section_has_no_hf_search() {
    let html = get_index_html();
    assert!(
        !html.contains("id=\"hf-search\""),
        "HF search input should be removed"
    );
    assert!(
        !html.contains("Search HF"),
        "Search HF label should be removed"
    );
}

#[test]
fn models_section_has_no_hf_refresh() {
    let html = get_index_html();
    assert!(
        !html.contains("id=\"hf-refresh\""),
        "HF refresh button should be removed"
    );
    assert!(
        !html.contains("Refresh HF"),
        "Refresh HF text should be removed"
    );
}

#[test]
fn models_section_has_no_registered_refresh() {
    let html = get_index_html();
    assert!(
        !html.contains("id=\"registered-refresh\""),
        "Registered refresh button should be removed"
    );
    assert!(
        !html.contains("Refresh Registered"),
        "Refresh Registered text should be removed"
    );
}

#[test]
fn models_section_has_no_download_tasks_refresh() {
    let html = get_index_html();
    assert!(
        !html.contains("id=\"download-tasks-refresh\""),
        "Download tasks refresh button should be removed"
    );
    assert!(
        !html.contains("Refresh Tasks"),
        "Refresh Tasks text should be removed"
    );
}

#[test]
fn models_section_has_no_section_tools() {
    let html = get_index_html();
    // The entire section-tools container should be removed from models section
    // Check that section-tools doesn't appear in the models tab context
    let models_section = html.find("id=\"tab-models\"").map(|start| {
        let end = html[start..].find("</div>").unwrap_or(html.len() - start);
        &html[start..start + end + 200] // Get enough context
    });

    if let Some(section) = models_section {
        assert!(
            !section.contains("section-tools"),
            "Models section should not have section-tools container"
        );
    }
}

#[test]
fn dashboard_has_no_japanese_text() {
    let html = get_index_html();
    assert!(
        !html.contains("対応可能モデル"),
        "Japanese text should be removed from models section"
    );
    assert!(
        !html.contains("対応モデル"),
        "Japanese text should be removed from models section"
    );
    assert!(
        !html.contains("テスト用コンソール"),
        "Japanese text should be removed from chat modal"
    );
}

/// ダッシュボードは i18n(EN/JA) 対応に移行したため、翻訳文字列(JS バンドル内の
/// ja.json)は日本語を含んでよい。一方、静的 HTML シェル（エントリ HTML）は
/// 言語非依存の英語のままに保つ（ローカライズは JS 側の i18n が担う）。
#[test]
fn dashboard_html_shells_have_no_japanese_text() {
    let html_paths = [
        "src/web/static/index.html",
        "src/web/static/login.html",
        "src/web/static/register.html",
        "src/web/static/change-password.html",
    ];

    for path in html_paths {
        let content = get_file_content(path);
        assert_no_japanese(&content, path);
    }
}
