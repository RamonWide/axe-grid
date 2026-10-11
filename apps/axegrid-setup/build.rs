//! Gives the installer the app icon on Windows.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/axe-grid.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/icon/axe-grid.ico");
        if let Err(error) = resource.compile() {
            // Without a resource compiler the program still works, with a plain icon.
            println!("cargo:warning=no icon embedded: {error}");
        }
    }
}
