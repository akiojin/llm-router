fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS")
        .map(|os| os != "windows")
        .unwrap_or(true)
    {
        return;
    }

    println!("cargo:rerun-if-changed=../assets/icons/coordinator.ico");

    let mut res = winresource::WindowsResource::new();
    res.set_icon("../assets/icons/coordinator.ico");
    res.compile()
        .expect("failed to embed coordinator Windows resources");
}
