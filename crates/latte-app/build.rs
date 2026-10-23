//! Puts the app's icon in the Windows .exe as icon 1, which GPUI gives its windows and
//! Explorer shows for the file.

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=icons/icon.ico");
        println!("cargo:rerun-if-changed=latte-books.rc");
        embed_resource::compile("latte-books.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
