//! The Typst world a template is compiled in: the template, Latte's print library
//! (`/latte.typ`), the values as `/data.json`, the document's images, and the fonts
//! bundled with Latte.
//!
//! Only bundled fonts are used, never the computer's, so a template prints the same on
//! every device.

use std::collections::HashMap;
use std::sync::LazyLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;

/// The fonts bundled with Latte (see `fonts/`): Liberation Sans, Serif and Mono, which
/// have the metrics of Arial, Times New Roman and Courier, the fonts Books' Print
/// Settings offer; Inter, the font of Books' screens; then Typst's own.
const BUNDLED: [&[u8]; 17] = [
    include_bytes!("../fonts/LiberationSans-Regular.ttf"),
    include_bytes!("../fonts/LiberationSans-Bold.ttf"),
    include_bytes!("../fonts/LiberationSans-Italic.ttf"),
    include_bytes!("../fonts/LiberationSans-BoldItalic.ttf"),
    include_bytes!("../fonts/LiberationSerif-Regular.ttf"),
    include_bytes!("../fonts/LiberationSerif-Bold.ttf"),
    include_bytes!("../fonts/LiberationSerif-Italic.ttf"),
    include_bytes!("../fonts/LiberationSerif-BoldItalic.ttf"),
    include_bytes!("../fonts/LiberationMono-Regular.ttf"),
    include_bytes!("../fonts/LiberationMono-Bold.ttf"),
    include_bytes!("../fonts/LiberationMono-Italic.ttf"),
    include_bytes!("../fonts/LiberationMono-BoldItalic.ttf"),
    include_bytes!("../fonts/Inter-Regular.otf"),
    include_bytes!("../fonts/Inter-Italic.otf"),
    include_bytes!("../fonts/Inter-Medium.otf"),
    include_bytes!("../fonts/Inter-SemiBold.otf"),
    include_bytes!("../fonts/Inter-Bold.otf"),
];

static FONTS: LazyLock<FontStore> = LazyLock::new(|| {
    let mut store = FontStore::new();
    for data in BUNDLED {
        for font in Font::iter(Bytes::new(data)) {
            let info = font.info().clone();
            store.push((font, info));
        }
    }
    store.extend(typst_kit::fonts::embedded());
    store
});

/// The families of the bundled fonts, as a template names them.
pub fn font_families() -> Vec<String> {
    let mut families: Vec<String> = FONTS
        .book()
        .families()
        .map(|(family, _)| family.to_owned())
        .collect();
    families.dedup();
    families
}

static LIBRARY: LazyLock<LazyHash<Library>> = LazyLock::new(|| LazyHash::new(Library::default()));

pub fn file_id(path: &str) -> FileId {
    let path = VirtualPath::new(path).expect("a valid virtual path");
    FileId::new(RootedPath::new(VirtualRoot::Project, path))
}

pub struct PrintWorld {
    main: FileId,
    sources: HashMap<FileId, Source>,
    files: HashMap<FileId, Bytes>,
}

impl PrintWorld {
    pub fn new(sources: &[(&str, &str)], files: Vec<(&str, Vec<u8>)>) -> Self {
        let sources: HashMap<FileId, Source> = sources
            .iter()
            .map(|(path, text)| {
                let id = file_id(path);
                (id, Source::new(id, (*text).to_owned()))
            })
            .collect();
        let files = files
            .into_iter()
            .map(|(path, data)| (file_id(path), Bytes::new(data)))
            .collect();
        Self {
            main: file_id(sources_main()),
            sources,
            files,
        }
    }
}

/// The template is always compiled as `/main.typ`.
pub fn sources_main() -> &'static str {
    "/main.typ"
}

impl World for PrintWorld {
    fn library(&self) -> &LazyHash<Library> {
        &LIBRARY
    }

    fn book(&self) -> &LazyHash<FontBook> {
        FONTS.book()
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.sources
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(format!("{id:?}").into()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(source) = self.sources.get(&id) {
            return Ok(Bytes::from_string(source.text().to_owned()));
        }
        self.files
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(format!("{id:?}").into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        FONTS.font(index)
    }

    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}
