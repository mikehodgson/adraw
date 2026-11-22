fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=assets/adraw.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").expect("Cargo target OS is required") == "windows" {
        winresource::WindowsResource::new()
            .set_icon("assets/adraw.ico")
            .set("ProductName", "ADraw")
            .set("FileDescription", "ADraw")
            .set("InternalName", "adraw")
            .set("OriginalFilename", "adraw.exe")
            .compile()?;
    }
    Ok(())
}
