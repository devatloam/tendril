fn main() {
    let config = slint_build::CompilerConfiguration::new().embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/app.slint", config).expect("the UI compiles");
    // CI sets this for preview builds (0.2.1-nightly.20261115.42); see update::VERSION.
    println!("cargo:rerun-if-env-changed=TENDRIL_VERSION");

    // Tendril's icon on the .exe, for Explorer and the taskbar. Built on Windows only: the
    // resource compiler is part of the Windows SDK.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        // ID 1: the tray icon (tray.rs) loads it by that number.
        resource.set_icon_with_id("assets/tendril.ico", "1");
        resource.compile().expect("the Windows icon compiles");
    }
}
