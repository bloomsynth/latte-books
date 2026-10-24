//! Puts the app's icon in the Windows .exe as icon 1, which GPUI gives its windows and
//! Explorer shows for the file, and its product name and version, which Explorer shows
//! under Details and code signing checks.

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=icons/icon.ico");
        println!("cargo:rerun-if-changed=latte-books.rc");
        // latte-books.rc holds the icon; the version block is made here from Cargo.toml.
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let out = std::env::var("OUT_DIR").unwrap();
        let icon = std::fs::read_to_string("latte-books.rc")
            .unwrap()
            .replace("icons/", &format!("{}/icons/", dir.replace('\\', "/")));
        let rc = format!("{icon}\n{}", version_info("Latte Books", "latte-books"));
        let path = format!("{out}/latte-books.rc");
        std::fs::write(&path, rc).unwrap();
        embed_resource::compile(&path, embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}

/// A VERSIONINFO resource for `file`.exe, with the version from Cargo.toml.
#[cfg(windows)]
fn version_info(description: &str, file: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let numeric = format!(
        "{},{},{},0",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    format!(
        r#"1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Bloomsynth"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{file}"
      VALUE "LegalCopyright", "Copyright (C) 2026 Bloomsynth. AGPL-3.0. Based on Frappe Books."
      VALUE "OriginalFilename", "{file}.exe"
      VALUE "ProductName", "Latte Books"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}
