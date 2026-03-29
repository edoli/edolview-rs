fn main() {
    println!("cargo:rerun-if-env-changed=EDOLVIEW_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_TYPE");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");

    let version = std::env::var("EDOLVIEW_BUILD_VERSION")
        .ok()
        .or_else(|| {
            let ref_type = std::env::var("GITHUB_REF_TYPE").ok()?;
            let ref_name = std::env::var("GITHUB_REF_NAME").ok()?;
            if ref_type == "tag" {
                Some(ref_name.trim_start_matches(['v', 'V']).to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION must be set"));

    println!("cargo:rustc-env=EDOLVIEW_BUILD_VERSION={version}");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        // Use the generated .ico
        res.set_icon("icons/app.ico");
        res.compile().expect("Failed to embed Windows icon");
    }
}
