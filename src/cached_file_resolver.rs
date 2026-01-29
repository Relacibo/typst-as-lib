use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
};

use typst::{
    diag::FileResult,
    foundations::Bytes,
    syntax::{FileId, Source},
};

use crate::file_resolver::FileResolver;

/// Wraps a file resolver with in-memory caching.
pub struct CachedFileResolver<T> {
    /// The underlying file resolver.
    pub file_resolver: T,
    /// Optional cache for source files.
    pub in_memory_source_cache: Option<Arc<Mutex<HashMap<FileId, Source>>>>,
    /// Optional cache for binary files.
    pub in_memory_binary_cache: Option<Arc<Mutex<HashMap<FileId, Bytes>>>>,
}

impl<T> CachedFileResolver<T> {
    /// Creates a new cached file resolver wrapping the given resolver.
    pub fn new(file_resolver: T) -> Self {
        CachedFileResolver {
            file_resolver,
            in_memory_source_cache: None,
            in_memory_binary_cache: None,
        }
    }

    /// Enables in-memory caching for source files.
    pub fn with_in_memory_source_cache(self) -> Self {
        Self {
            in_memory_source_cache: Some(Default::default()),
            ..self
        }
    }

    /// Enables in-memory caching for binary files.
    pub fn with_in_memory_binary_cache(self) -> Self {
        Self {
            in_memory_binary_cache: Some(Default::default()),
            ..self
        }
    }
}

impl<T> FileResolver for CachedFileResolver<T>
where
    T: FileResolver,
{
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<'_, Bytes>> {
        let Self {
            in_memory_binary_cache,
            ..
        } = self;

        if let Some(in_memory_binary_cache) = in_memory_binary_cache
            && let Ok(in_memory_binary_cache) = in_memory_binary_cache.lock()
            && let Some(cached) = in_memory_binary_cache.get(&id)
        {
            return Ok(Cow::Owned(cached.clone()));
        }
        let resolved = self.file_resolver.resolve_binary(id)?;
        if let Some(in_memory_binary_cache) = in_memory_binary_cache
            && let Ok(mut in_memory_binary_cache) = in_memory_binary_cache.lock()
        {
            in_memory_binary_cache.insert(id, resolved.as_ref().clone());
        }
        Ok(resolved)
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<'_, Source>> {
        let Self {
            in_memory_source_cache,
            ..
        } = self;

        if let Some(in_memory_source_cache) = in_memory_source_cache
            && let Ok(in_memory_source_cache) = in_memory_source_cache.lock()
            && let Some(cached) = in_memory_source_cache.get(&id)
        {
            return Ok(Cow::Owned(cached.clone()));
        }
        let resolved = self.file_resolver.resolve_source(id)?;
        if let Some(in_memory_source_cache) = in_memory_source_cache
            && let Ok(mut in_memory_source_cache) = in_memory_source_cache.lock()
        {
            in_memory_source_cache.insert(id, resolved.as_ref().clone());
        }
        Ok(resolved)
    }
}

/// Trait for converting a file resolver into a cached version.
pub trait IntoCachedFileResolver {
    /// Wraps this resolver with caching.
    fn into_cached(self) -> CachedFileResolver<Self>
    where
        Self: Sized;
}
