fn main() {
    let proxy_acl = tauri_build::InlinedPlugin::new()
        .commands(&[
            "proxy_self_test",
            "proxy_self_test_all",
            "proxy_self_test_report",
            "get_proxy_latency_snapshot",
            "get_best_proxy_candidate",
            "remove_proxy_latency_entry",
            "clear_proxy_latency_data",
            "get_proxy_latency_entry",
            "get_proxy_latency_score",
        ])
        .default_permission(tauri_build::DefaultPermissionRule::AllowAllCommands);

    let attributes = tauri_build::Attributes::new().plugin("proxysec", proxy_acl);
    #[cfg(windows)]
    let attributes = {
        add_manifest();
        attributes.windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest())
    };
    tauri_build::try_build(attributes).unwrap();
}

#[cfg(windows)]
fn add_manifest() {
    static WINDOWS_MANIFEST_FILE: &str = "windows-app-manifest.xml";

    let manifest = std::env::current_dir().unwrap().join(WINDOWS_MANIFEST_FILE);

    println!("cargo:rerun-if-changed={}", manifest.display());
    // Embed the Windows application manifest file.
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
        manifest.to_str().unwrap()
    );
    // Turn linker warnings into errors.
    println!("cargo:rustc-link-arg=/WX");
}
