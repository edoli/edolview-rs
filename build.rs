fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        // Use the generated .ico
        res.set_icon("icons/app.ico");
        res.compile().expect("Failed to embed Windows icon");
    }
}
