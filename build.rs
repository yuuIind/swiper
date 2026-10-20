// Embeds the application icon and version info into swiper.exe on Windows.
fn main() {
    println!("cargo:rerun-if-changed=assets/swiper.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/swiper.ico");
        res.set("FileDescription", "swiper - image dataset sorter");
        res.set("ProductName", "swiper");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the icon: {e}");
        }
    }
}
