#[derive(Clone, Debug)]
pub struct FontSearcherOptions<I = [std::path::PathBuf; 0], P = std::path::PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<std::path::Path>,
{
    pub(crate) include_system_fonts: bool,
    pub(crate) include_dirs: I,
    #[cfg(feature = "typst-kit-embed-fonts")]
    pub(crate) include_embedded_fonts: bool,
}

impl Default for FontSearcherOptions<[std::path::PathBuf; 0], std::path::PathBuf> {
    fn default() -> Self {
        Self {
            include_system_fonts: true,
            include_dirs: Default::default(),
            #[cfg(feature = "typst-kit-embed-fonts")]
            include_embedded_fonts: true,
        }
    }
}

impl FontSearcherOptions<[std::path::PathBuf; 0], std::path::PathBuf> {
    pub fn new() -> Self {
        FontSearcherOptions::default()
    }
}

impl<I, P> FontSearcherOptions<I, P>
where
    I: IntoIterator<Item = P>,
    P: AsRef<std::path::Path>,
{
    pub fn include_system_fonts(mut self, include_system_fonts: bool) -> Self {
        self.include_system_fonts = include_system_fonts;
        self
    }

    #[cfg(feature = "typst-kit-embed-fonts")]
    pub fn include_embedded_fonts(mut self, include_embedded_fonts: bool) -> Self {
        self.include_embedded_fonts = include_embedded_fonts;
        self
    }

    pub fn include_dirs<I2, P2>(self, include_dirs: I2) -> FontSearcherOptions<I2, P2>
    where
        I2: IntoIterator<Item = P2>,
        P2: AsRef<std::path::Path>,
    {
        FontSearcherOptions::<I2, P2> {
            include_system_fonts: self.include_system_fonts,
            include_dirs,
            #[cfg(feature = "typst-kit-embed-fonts")]
            include_embedded_fonts: self.include_embedded_fonts,
        }
    }
}
