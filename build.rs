fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/opon.ico");
        winresource::WindowsResource::new()
            .set_icon("assets/opon.ico")
            .set("ProductName", "OPON")
            .set("FileDescription", "OPON Notes Workspace")
            .compile()
            .expect("embed Windows application icon");
    }
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/main.slint", config).expect("compile OPON interface");
}
