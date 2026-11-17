//! Diagnostics that stay displayable after compilation.
//!
//! Compilation errors returned by this crate carry the raw Typst
//! [`SourceDiagnostic`]s wrapped in a [`SourceDiagnostics`] (see
//! [`TypstAsLibError::TypstSource`](crate::TypstAsLibError::TypstSource)). The
//! wrapper additionally keeps the [`Source`]s the diagnostics' spans point
//! into, collected while the [`World`] is still alive. That way locations —
//! file, line and column — can be resolved lazily, *without* a world, via
//! [`SourceDiagnostics::location`], and [`SourceDiagnostics`] renders a
//! human-readable report through its [`Display`](std::fmt::Display)
//! implementation.
//!
//! When driving compilation yourself through
//! [`TypstEngine::with_world`](crate::TypstEngine::with_world) or
//! [`TypstEngine::world_builder`](crate::TypstEngine::world_builder), errors
//! *and* warnings come back as raw [`SourceDiagnostic`]s. Call
//! [`SourceDiagnostics::new`] while the world is alive to get the same
//! location-aware wrapper, e.g. for warnings:
//!
//! ```rust,no_run
//! # use typst_as_lib::{TypstEngine, diagnostic::SourceDiagnostics};
//! # use typst_layout::PagedDocument;
//! static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
//!
//! let engine = TypstEngine::builder().main_file("= Hello").fonts([FONT]).build();
//!
//! let report = engine.with_world(|world| {
//!     let warned = typst::compile::<PagedDocument>(world);
//!     let warnings = SourceDiagnostics::new(world, warned.warnings);
//!     warnings.to_string()
//! })
//! .unwrap();
//! # let _ = report;
//! ```
use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, Range};

use ecow::{EcoString, EcoVec, eco_format};
use typst::World;
use typst::diag::{Severity, SourceDiagnostic};
use typst::syntax::{DiagSpan, DiagSpanKind, FileId, Source, VirtualRoot};

/// Typst diagnostics together with the [`Source`]s their spans point into.
///
/// Wraps the raw diagnostics — accessible through
/// `Deref<Target = [SourceDiagnostic]>`, so existing code like `diags.iter()`
/// or `diag.message` keeps working — and makes locations resolvable without a
/// [`World`]: [`location()`](Self::location) returns the file, line and
/// column of a span, and the [`Display`](std::fmt::Display) implementation
/// renders a complete human-readable report.
///
/// Collected with [`SourceDiagnostics::new`] while a [`World`] is available —
/// in this crate when a compilation fails, or manually inside
/// [`with_world`](crate::TypstEngine::with_world), e.g. for warnings.
#[derive(Debug, Clone)]
pub struct SourceDiagnostics {
    diagnostics: EcoVec<SourceDiagnostic>,
    sources: HashMap<FileId, Source>,
}

impl SourceDiagnostics {
    /// Collects `diagnostics` together with the [`Source`]s referenced by
    /// their spans: the diagnostic's own span, its traceback spans and its
    /// hint spans.
    ///
    /// Sources that cannot be resolved from the world are skipped; locations
    /// pointing into them simply won't resolve later. Call this while the
    /// world is alive — see the [module documentation](self) for the
    /// `with_world` pattern.
    pub fn new<W: World + ?Sized>(world: &W, diagnostics: EcoVec<SourceDiagnostic>) -> Self {
        let mut sources = HashMap::new();
        for diag in &diagnostics {
            collect_source(&mut sources, world, diag.span);
            for traced in &diag.trace {
                collect_source(&mut sources, world, traced.span);
            }
            for hint in &diag.hints {
                collect_source(&mut sources, world, hint.span);
            }
        }
        Self {
            diagnostics,
            sources,
        }
    }

    /// Resolves the location of a span: the file and 1-based line/column
    /// numbers of its start and end.
    ///
    /// Returns `None` when the location cannot be resolved, e.g. for detached
    /// spans or files that were not collected. Accepts a
    /// [`Span`](typst::syntax::Span) or a [`DiagSpan`]; the spans of raw
    /// diagnostics are the latter, the ones in a traceback the former.
    pub fn location(&self, span: impl Into<DiagSpan>) -> Option<Location> {
        let (source, range) = self.resolve(span.into())?;
        let lines = source.lines();
        let (line, column) = lines.byte_to_line_column(range.start)?;
        let (end_line, end_column) = lines.byte_to_line_column(range.end)?;
        Some(Location {
            file: file_name(source.id()),
            line: line + 1,
            column: column + 1,
            end_line: end_line + 1,
            end_column: end_column + 1,
        })
    }

    /// The collected [`Source`] for a file, for rendering diagnostics
    /// yourself (e.g. with a code renderer like `codespan-reporting`).
    pub fn source(&self, id: FileId) -> Option<&Source> {
        self.sources.get(&id)
    }

    /// The raw diagnostics, without the collected sources.
    pub fn into_inner(self) -> EcoVec<SourceDiagnostic> {
        self.diagnostics
    }

    /// Resolves a diagnostic span into a source and its byte range,
    /// mirroring typst's own `WorldExt::range`, but against the collected
    /// sources instead of a world. Never panics: unresolvable spans resolve
    /// to `None`.
    fn resolve(&self, span: DiagSpan) -> Option<(&Source, Range<usize>)> {
        match span.get() {
            DiagSpanKind::Detached => None,
            DiagSpanKind::Number { id, num, sub_range } => {
                let source = self.sources.get(&id)?;
                let range = source.range(num, sub_range)?;
                Some((source, range))
            }
            DiagSpanKind::Range { id, range } => {
                let source = self.sources.get(&id)?;
                let end = range.end.min(source.lines().len_bytes());
                (range.start <= end).then_some((source, range.start..end))
            }
        }
    }
}

/// Collects the source file for a span's file id, if not already present and
/// resolvable from the world.
fn collect_source(
    sources: &mut HashMap<FileId, Source>,
    world: &(impl World + ?Sized),
    span: impl Into<DiagSpan>,
) {
    let Some(id) = span.into().id() else {
        return;
    };
    if sources.contains_key(&id) {
        return;
    }
    if let Ok(source) = world.source(id) {
        sources.insert(id, source);
    }
}

impl Deref for SourceDiagnostics {
    type Target = [SourceDiagnostic];

    fn deref(&self) -> &Self::Target {
        &self.diagnostics
    }
}

impl fmt::Display for SourceDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diag) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            let severity = match diag.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            write!(f, "{severity}: {}", diag.message)?;
            if let Some(location) = self.location(diag.span) {
                write!(f, " (at {location})")?;
            }
            if !diag.trace.is_empty() {
                write!(f, "\ntraceback:")?;
                for traced in &diag.trace {
                    write!(f, "\n  {}", traced.v)?;
                    if let Some(location) = self.location(traced.span) {
                        write!(f, " (at {location})")?;
                    }
                }
            }
            if !diag.hints.is_empty() {
                write!(f, "\nhints:")?;
                for hint in &diag.hints {
                    write!(f, "\n  {}", hint.v)?;
                }
            }
        }
        Ok(())
    }
}

/// The location of a span: a file path and 1-based line/column numbers for
/// its start and end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The file the span points into. Project files are shown relative to
    /// the project root (e.g. `main.typ`), package files with their package
    /// spec (e.g. `@preview/example:0.1.0/lib.typ`).
    pub file: EcoString,
    /// The 1-based line of the span's start.
    pub line: usize,
    /// The 1-based column of the span's start.
    pub column: usize,
    /// The 1-based line of the span's end.
    pub end_line: usize,
    /// The 1-based column of the span's end.
    pub end_column: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)?;
        if (self.line, self.column) != (self.end_line, self.end_column) {
            write!(f, "-{}:{}", self.end_line, self.end_column)?;
        }
        Ok(())
    }
}

/// Renders a file id as a user-facing path: project files relative to the
/// project root, package files prefixed with their package spec.
fn file_name(id: FileId) -> EcoString {
    let path = id.get();
    let name = path.vpath().get_without_slash();
    match path.root() {
        VirtualRoot::Project => EcoString::from(name),
        VirtualRoot::Package(package) => eco_format!("{package}/{name}"),
    }
}
