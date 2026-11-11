fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config).unwrap();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        // The signing service checks these against the project name.
        res.set("ProductName", "Nightscout Glance");
        res.set("FileDescription", "Nightscout Glance");
        res.set("LegalCopyright", "Benedikt Reiser, MIT License");
        res.compile().unwrap();
    }
}
