use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct TypstKitFontOptions {
    pub(crate) include_system_fonts: bool,
    pub(crate) include_dirs: Vec<PathBuf>,
    #[cfg(feature = "typst-kit-embed-fonts")]
    pub(crate) include_embedded_fonts: bool,
}

impl Default for TypstKitFontOptions {
    fn default() -> Self {
        Self {
            include_system_fonts: true,
            include_dirs: Default::default(),
            #[cfg(feature = "typst-kit-embed-fonts")]
            include_embedded_fonts: true,
        }
    }
}

impl TypstKitFontOptions {
    pub fn new() -> Self {
        TypstKitFontOptions::default()
    }
}

impl TypstKitFontOptions {
    pub fn include_system_fonts(mut self, include_system_fonts: bool) -> Self {
        self.include_system_fonts = include_system_fonts;
        self
    }

    #[cfg(feature = "typst-kit-embed-fonts")]
    pub fn include_embedded_fonts(mut self, include_embedded_fonts: bool) -> Self {
        self.include_embedded_fonts = include_embedded_fonts;
        self
    }

    pub fn include_dirs<I, P>(mut self, include_dirs: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<std::path::PathBuf>,
    {
        self.include_dirs = include_dirs.into_iter().map(Into::into).collect();
        self
    }
}
