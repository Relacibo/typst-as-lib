//! Time sources for Typst's `datetime.today()`.
//!
//! In a Typst document, `datetime.today()` resolves through
//! [`typst::World::today`], which typst-as-lib backs with a
//! [`TimeProvider`]. The provider is queried once per compilation, so all
//! `datetime.today()` calls within one document see the same time.
//!
//! The default provider is [`SystemTimeProvider`]. Use
//! [`time_provider`](crate::TypstTemplateEngineBuilder::time_provider) to
//! install a custom one, or
//! [`with_fixed_time`](crate::TypstWorldBuilder::with_fixed_time) to override it for a
//! single compilation.
//!
//! Times outside the range supported by the `time` crate (years -9999 to
//! 9999) are treated as unavailable: `datetime.today()` then reports a
//! normal error instead of the compilation panicking.

use std::sync::Arc;
use std::time::SystemTime;

/// Provides the current time for Typst documents.
///
/// # Example
///
/// A provider reading [`SOURCE_DATE_EPOCH`](https://reproducible-builds.org/docs/source-date-epoch/),
/// e.g. for reproducible builds. If the variable is missing or unparsable,
/// the system time is used:
///
/// ```
/// # use std::time::{Duration, SystemTime};
/// # use typst_as_lib::{TypstEngine, time::TimeProvider};
///
/// struct SourceDateEpoch;
///
/// impl TimeProvider for SourceDateEpoch {
///     fn now(&self) -> SystemTime {
///         std::env::var("SOURCE_DATE_EPOCH")
///             .ok()
///             .and_then(|epoch| epoch.parse::<u64>().ok())
///             .map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
///             .unwrap_or_else(SystemTime::now)
///     }
/// }
///
/// static TEMPLATE: &str = "#datetime.today().display()";
/// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
///
/// let engine = TypstEngine::builder()
///     .main_file(TEMPLATE)
///     .fonts([FONT])
///     .time_provider(SourceDateEpoch)
///     .build();
///
/// let world = engine.world_builder().build().unwrap();
/// ```
///
/// Stateful clocks (e.g. a steerable test clock) can be shared across
/// engines: [`Arc`] and [`Box`] implement [`TimeProvider`] via blanket
/// impls, so pass `Arc::clone(&clock)` and keep your own handle.
pub trait TimeProvider: Send + Sync {
    /// Returns the current time.
    fn now(&self) -> SystemTime;
}

/// Default [`TimeProvider`], returning the current system time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTimeProvider;

impl TimeProvider for SystemTimeProvider {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// A [`TimeProvider`] that always returns the same fixed time.
///
/// Useful for reproducible output, e.g. snapshot tests or deterministic
/// builds of documents that use `datetime.today()`. For a one-off override
/// see [`with_fixed_time`](crate::TypstWorldBuilder::with_fixed_time).
///
/// # Example
///
/// ```
/// # use std::time::{Duration, SystemTime};
/// # use typst_as_lib::{TypstEngine, time::FixedTimeProvider};
///
/// static TEMPLATE: &str = "#datetime.today().display()";
/// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
///
/// let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(1_718_452_800);
/// let engine = TypstEngine::builder()
///     .main_file(TEMPLATE)
///     .fonts([FONT])
///     .time_provider(FixedTimeProvider::new(fixed))
///     .build();
///
/// let world = engine.world_builder().build().unwrap();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct FixedTimeProvider {
    now: SystemTime,
}

impl FixedTimeProvider {
    /// Creates a provider that always returns `now`.
    pub fn new(now: SystemTime) -> Self {
        Self { now }
    }
}

impl TimeProvider for FixedTimeProvider {
    fn now(&self) -> SystemTime {
        self.now
    }
}

/// Shared ownership: `Arc<T>` delegates to `T`, so a stateful clock can be
/// kept by the caller and shared across engines.
impl<T: TimeProvider + ?Sized> TimeProvider for Arc<T> {
    fn now(&self) -> SystemTime {
        (**self).now()
    }
}

/// Owned dynamic dispatch: `Box<T>` delegates to `T`.
impl<T: TimeProvider + ?Sized> TimeProvider for Box<T> {
    fn now(&self) -> SystemTime {
        (**self).now()
    }
}
