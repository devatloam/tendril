fn main() {
    let config = slint_build::CompilerConfiguration::new().embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/app.slint", config).expect("the UI compiles");

    // Tendril's icon on the .exe, for Explorer and the taskbar. Built on Windows only: the
    // resource compiler is part of the Windows SDK.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/tendril.ico");
        resource.compile().expect("the Windows icon compiles");
    }
}
