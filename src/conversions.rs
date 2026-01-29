use typst::{
    foundations::Bytes,
    syntax::{FileId, Source, VirtualPath, package::PackageSpec},
    text::Font,
};

/// Converts types into a Typst [`FileId`].
pub trait IntoFileId {
    /// Converts into a file ID.
    fn into_file_id(self) -> FileId
    where
        Self: std::marker::Sized;
}

impl IntoFileId for FileId {
    fn into_file_id(self) -> FileId {
        self
    }
}

impl IntoFileId for &str {
    fn into_file_id(self) -> FileId {
        FileId::new(None, VirtualPath::new(self))
    }
}

impl IntoFileId for (PackageSpec, &str) {
    fn into_file_id(self) -> FileId {
        let (p, id) = self;
        FileId::new(Some(p), VirtualPath::new(id))
    }
}

/// Converts types into a Typst [`Source`].
pub trait IntoSource {
    /// Converts into a source file.
    fn into_source(self) -> Source
    where
        Self: std::marker::Sized;
}

impl IntoSource for Source {
    fn into_source(self) -> Source {
        self
    }
}

impl IntoSource for (&str, String) {
    fn into_source(self) -> Source {
        let (path, source) = self;
        let id = FileId::new(None, VirtualPath::new(path));
        Source::new(id, source)
    }
}

impl IntoSource for (&str, &str) {
    fn into_source(self) -> Source {
        let (path, source) = self;
        (path, source.to_owned()).into_source()
    }
}

impl IntoSource for (FileId, String) {
    fn into_source(self) -> Source {
        let (id, source) = self;
        Source::new(id, source)
    }
}

impl IntoSource for (FileId, &str) {
    fn into_source(self) -> Source {
        let (id, source) = self;
        Source::new(id, source.to_owned())
    }
}

impl IntoSource for String {
    fn into_source(self) -> Source {
        Source::detached(self)
    }
}

impl IntoSource for &str {
    fn into_source(self) -> Source {
        Source::detached(self.to_owned())
    }
}

/// Converts types into Typst [`Bytes`].
pub trait IntoBytes {
    /// Converts into bytes.
    fn into_bytes(self) -> Bytes
    where
        Self: std::marker::Sized;
}

impl IntoBytes for &[u8] {
    fn into_bytes(self) -> Bytes {
        Bytes::new(self.to_vec())
    }
}

impl IntoBytes for Vec<u8> {
    fn into_bytes(self) -> Bytes {
        Bytes::new(self)
    }
}

impl IntoBytes for Bytes {
    fn into_bytes(self) -> Bytes {
        self
    }
}

/// Converts types into an iterator of Typst [`Font`]s.
pub trait IntoFonts
where
    Self: std::marker::Sized,
{
    /// Converts into an iterator of fonts.
    fn into_fonts(self) -> Box<dyn Iterator<Item = Font>>;
}

impl IntoFonts for &[u8] {
    fn into_fonts(self) -> Box<dyn Iterator<Item = Font>> {
        Box::new(Font::iter(Bytes::new(self.to_vec())))
    }
}

impl IntoFonts for Vec<u8> {
    fn into_fonts(self) -> Box<dyn Iterator<Item = Font>> {
        Box::new(Font::iter(Bytes::new(self)))
    }
}

impl IntoFonts for Font {
    fn into_fonts(self) -> Box<dyn Iterator<Item = Font>> {
        Box::new(std::iter::once(self))
    }
}

impl IntoFonts for Bytes {
    fn into_fonts(self) -> Box<dyn Iterator<Item = Font>> {
        Box::new(Font::iter(self))
    }
}
