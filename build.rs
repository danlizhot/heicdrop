// Intègre l'icône et les informations de version dans l'exécutable Windows.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "HeicDrop")
            .set("FileDescription", "HeicDrop - convertisseur d'images HEIC")
            .set("LegalCopyright", "Licence MIT");
        if let Err(e) = res.compile() {
            println!("cargo:warning=ressources Windows non intégrées : {e}");
        }
    }
}
