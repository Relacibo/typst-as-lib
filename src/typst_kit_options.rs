use std::path::PathBuf;

/// Configuration options for `typst-kit` font searching.
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
    /// Creates new options with default settings.
    pub fn new() -> Self {
        TypstKitFontOptions::default()
    }
}

impl TypstKitFontOptions {
    /// Sets whether to include system fonts.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::typst_kit_options::TypstKitFontOptions;
    /// let options = TypstKitFontOptions::new()
    ///     .include_system_fonts(true);
    /// ```
    pub fn include_system_fonts(mut self, include_system_fonts: bool) -> Self {
        self.include_system_fonts = include_system_fonts;
        self
    }

    /// Sets whether to include embedded fonts from `typst-assets`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::typst_kit_options::TypstKitFontOptions;
    /// let options = TypstKitFontOptions::new()
    ///     .include_embedded_fonts(true);
    /// ```
    #[cfg(feature = "typst-kit-embed-fonts")]
    pub fn include_embedded_fonts(mut self, include_embedded_fonts: bool) -> Self {
        self.include_embedded_fonts = include_embedded_fonts;
        self
    }

    /// Adds additional directories to search for fonts.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::typst_kit_options::TypstKitFontOptions;
    /// let options = TypstKitFontOptions::new()
    ///     .include_dirs(["./fonts", "./custom-fonts"]);
    /// ```
    pub fn include_dirs<I, P>(mut self, include_dirs: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<std::path::PathBuf>,
    {
        self.include_dirs = include_dirs.into_iter().map(Into::into).collect();
        self
    }
}
