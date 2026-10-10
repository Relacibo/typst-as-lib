//! Convert world dependent [`SourceDiagnositcs`] into independently displayable Diagnostics.
use std::{fmt::Display, ops::Range};

use ecow::{EcoString, EcoVec};
use typst::{
    diag::{Severity, SourceDiagnostic, Tracepoint},
    syntax::{Source, Span},
};

#[derive(Debug, Clone)]
/// A new-type wrapper for displaying multiple diagnostics from a single compilation
pub struct Diagnostics(pub EcoVec<Diagnostic>);

impl std::fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let diags = &self.0;
        match diags.len() {
            0 => Ok(()),
            1 => write!(f, "{}", diags[0]),
            _ => {
                for (i, diag) in diags.iter().enumerate() {
                    write!(f, "{}", diag)?;
                    // do not write a newline for the last diagnostic
                    if i != diags.len() - 1 {
                        writeln!(f)?;
                    }
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone)]
/// A source diagnostic with a pretty-printable error message.
///
/// This is basically a [`SourceDiagnostic`] where [`Spans`] have been converted to [`ResolvedSpans`],
/// so that diagnostic information can be obtained without access to the [`typst::World`] used for compilation.
/// 
/// A displayed diagnostic will look like this:
/// ```txt
/// Error: panicked with: "invalid color. only green is allowed.", rgb("#ff4136") (at \function.typ:3:2-3:54)
/// traceback:
///   error occurred in this call of function `alert` (at \main.typ:7:1-7:14)
/// hints:
///   maybe choose a better color?
/// ```
pub struct Diagnostic {
    /// Whether the diagnostic is an error or a warning.
    pub severity: Severity,
    /// A diagnostic message describing the problem.
    pub message: EcoString,
    /// The span of the relevant node in the source code.
    pub span: Option<ResolvedSpan>,
    /// The trace of function calls leading to the problem.
    pub trace: EcoVec<(Option<ResolvedSpan>, Tracepoint)>,
    /// Additional hints to the user, indicating how this problem could be avoided
    /// or worked around.
    pub hints: EcoVec<EcoString>,
}

impl Diagnostic {
    /// Construct a [`Diagnostic`] from a [`SourceDiagnostic`] and the [`typst::World`] used during the compilation.
    pub fn from_source_diagnostic<W: typst::World>(world: &W, diag: SourceDiagnostic) -> Self {
        Self {
            severity: diag.severity,
            message: diag.message,
            span: ResolvedSpan::from_span(world, diag.span),
            trace: diag
                .trace
                .into_iter()
                .map(|spanned| (ResolvedSpan::from_span(world, spanned.span), spanned.v))
                .collect(),
            hints: diag.hints,
        }
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.severity, self.message)?;
        if let Some(span) = &self.span {
            write!(f, " (at {span})")?;
        }
        if !self.trace.is_empty() {
            write!(f, "\ntraceback:")?;
        }
        for (span, trace) in &self.trace {
            write!(f, "\n  {}", trace)?;
            if let Some(span) = span {
                write!(f, " (at {})", span)?;
            }
        }
        if !self.hints.is_empty() {
            write!(f, "\nhints:")?;
        }
        for hint in &self.hints {
            write!(f, "\n  {}", hint)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
/// A [`Span`] whose location in the source code has been resolved, so that it does **not** require a [`typst::World`] to obtain human readable information.
pub struct ResolvedSpan {
    /// The byte range of the given span in the source file
    pub range: Range<usize>,
    /// The source file for the given span
    pub source: Source,
}

impl ResolvedSpan {
    /// Resolves a [`typst::syntax::Span`] into a [`ResolvedSpan`] by obtaining the source from the world.
    ///
    /// Will return `None` if the file of the span is not found.
    pub fn from_span<W: typst::World>(world: &W, span: Span) -> Option<Self> {
        let file_id = span.id()?;
        let source = world.source(file_id).ok()?;
        // We obtained the source from the span, therefore the span is guaranteed to belong to the source file
        let range = source
            .range(span)
            .expect("span belongs to this source file");
        Some(Self { range, source })
    }

    fn location(&self) -> Location {
        let lines = self.source.lines();
        // The range and lines belong to the same source, so it is safe to unwrap
        let start_line = lines
            .byte_to_line(self.range.start)
            .expect("valid byte range");
        let start_col = lines
            .byte_to_column(self.range.start)
            .expect("valid byte range");
        let end_line = lines
            .byte_to_line(self.range.end)
            .expect("valid byte range");
        let end_col = lines
            .byte_to_column(self.range.end)
            .expect("valid byte range");
        Location {
            start_line,
            end_line,
            start_col,
            end_col,
        }
    }
}

impl Display for ResolvedSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}:{}", self.source.id().vpath(), self.location())
    }
}

#[derive(Debug, Clone)]
/// The location of a [`ResolvedSpan`] in the source file.
pub struct Location {
    /// The starting line of the span
    pub start_line: usize,
    /// The ending line of the span
    pub end_line: usize,
    /// The starting column of the span
    pub start_col: usize,
    /// The ending column of the span
    pub end_col: usize,
}

impl Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start_line == self.end_line && self.start_col == self.end_col {
            write!(f, "{}:{}", self.start_line, self.start_col)
        } else {
            write!(
                f,
                "{}:{}-{}:{}",
                self.start_line, self.start_col, self.end_line, self.end_col
            )
        }
    }
}
