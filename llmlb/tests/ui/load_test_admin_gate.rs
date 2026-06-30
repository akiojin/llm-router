// Load Test を admin ロール限定にするフロント実装のソースレベル回帰テスト。
//
// 目的: LB Playground の Load Test が UI/API 双方で admin 限定であることを、
// コンポーネントソースの構造で担保する（viewer には Load Test トグル非表示・
// startLoadTest ガード・admin 限定エンドポイントへの送信）。

fn lb_playground_source() -> String {
    include_str!("../../src/web/dashboard/src/pages/LoadBalancerPlayground.tsx").to_string()
}

fn chat_api_source() -> String {
    include_str!("../../src/web/dashboard/src/lib/api/chat.ts").to_string()
}

#[test]
fn lb_playground_derives_is_admin_from_auth() {
    let source = lb_playground_source();
    assert!(
        source.contains("useAuth") && source.contains("user?.role === 'admin'"),
        "LoadBalancerPlayground should derive isAdmin from useAuth"
    );
}

#[test]
fn load_test_mode_toggle_is_admin_gated() {
    let source = lb_playground_source();
    // Load Test モードトグルが isAdmin 条件下でのみ描画されること
    assert!(
        source.contains("{isAdmin && (") && source.contains("id=\"lb-mode-load-test\""),
        "Load Test mode toggle must be rendered only when isAdmin"
    );
}

#[test]
fn start_load_test_guards_on_admin() {
    let source = lb_playground_source();
    assert!(
        source.contains("if (!isAdmin"),
        "startLoadTest must bail out for non-admin users"
    );
}

#[test]
fn load_test_uses_admin_only_endpoint() {
    let lb = lb_playground_source();
    assert!(
        lb.contains("chatApi.completeLoadTest("),
        "load test worker must call the admin-only completeLoadTest"
    );

    let chat = chat_api_source();
    assert!(
        chat.contains("completeLoadTest")
            && chat.contains("/api/dashboard/playground/load-test/chat/completions"),
        "chat API must expose completeLoadTest targeting the admin-only load-test endpoint"
    );
    // 通常 Chat は従来の全ユーザー向けエンドポイントのまま
    assert!(
        chat.contains("/api/dashboard/playground/chat/completions"),
        "regular chat endpoint must remain for all users"
    );
}
