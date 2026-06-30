fn get_endpoint_table_source() -> String {
    include_str!("../../src/web/dashboard/src/components/dashboard/EndpointTable.tsx").to_string()
}

fn get_models_table_source() -> String {
    include_str!("../../src/web/dashboard/src/components/dashboard/ModelsTable.tsx").to_string()
}

#[test]
fn endpoint_table_does_not_render_tps_column() {
    let source = get_endpoint_table_source();
    assert!(
        !source.contains("handleSort('tps')"),
        "EndpointTable should not sort by TPS because endpoint-level TPS is not shown"
    );
    assert!(
        !source.contains("Aggregated endpoint TPS is hidden"),
        "EndpointTable should not include endpoint TPS placeholder guidance text"
    );
}

#[test]
fn models_table_renders_endpoint_model_tps() {
    let source = get_models_table_source();
    assert!(
        source.contains("queryKey: ['endpoint-model-tps', endpoint.id]"),
        "ModelsTable should fetch endpoint x model TPS data"
    );
    assert!(
        source.contains("TPS: ${modelTpsSummary}"),
        "ModelsTable expanded endpoint row should display TPS summary"
    );
}
