fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        winresource::WindowsResource::new()
            .set_icon("assets/app.ico")
            .set("FileDescription", "iPhone Bridge")
            .set("ProductName", "iPhone Bridge")
            .set("OriginalFilename", "iMessage.exe")
            .compile()
            .expect("Windows icon resource must be embedded");
    }
}
