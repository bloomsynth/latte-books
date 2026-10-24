//! Puts the product name and version in the Windows .exe, which Explorer shows under
//! Details and code signing checks. latte-app's build.rs does the same for the app.

fn main() {
    #[cfg(windows)]
    {
        let out = std::env::var("OUT_DIR").unwrap();
        let version = env!("CARGO_PKG_VERSION");
        let numeric = format!(
            "{},{},{},0",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR"),
            env!("CARGO_PKG_VERSION_PATCH")
        );
        let rc = format!(
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
      VALUE "FileDescription", "Dime, the Latte Books server for AI assistants"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "latte-mcp"
      VALUE "LegalCopyright", "Copyright (C) 2026 Bloomsynth. AGPL-3.0. Based on Frappe Books."
      VALUE "OriginalFilename", "latte-mcp.exe"
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
        );
        let path = format!("{out}/latte-mcp.rc");
        std::fs::write(&path, rc).unwrap();
        embed_resource::compile(&path, embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
