// Metadati dell'eseguibile (nome prodotto, versione, copyright): richiesti da SignPath
// e mostrati nelle proprietà del file e nella finestra di SmartScreen.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "SSAS AI Installer")
            .set("FileDescription", "Installazione guidata di SSAS AI")
            .set("CompanyName", "Lucio Gambelunghe")
            .set("LegalCopyright", "Copyright (c) 2026 Lucio Gambelunghe")
            .set("OriginalFilename", "ssas-installer.exe")
            .set("InternalName", "ssas-installer");
        res.compile().expect("impossibile incorporare i metadati dell'eseguibile");
    }
}
