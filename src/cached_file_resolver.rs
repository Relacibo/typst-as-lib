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

pub struct CachedFileResolver<T> {
    pub file_resolver: T,
    pub in_memory_source_cache: Option<Arc<Mutex<HashMap<FileId, Source>>>>,
    pub in_memory_binary_cache: Option<Arc<Mutex<HashMap<FileId, Bytes>>>>,
}

impl<T> CachedFileResolver<T> {
    pub fn new(file_resolver: T) -> Self {
        CachedFileResolver {
            file_resolver,
            in_memory_source_cache: None,
            in_memory_binary_cache: None,
        }
    }

    pub fn with_in_memory_source_cache(self) -> Self {
        Self {
            in_memory_source_cache: Some(Default::default()),
            ..self
        }
    }

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
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<Bytes>> {
        let Self {
            in_memory_binary_cache,
            ..
        } = self;

        if let Some(in_memory_binary_cache) = in_memory_binary_cache {
            if let Ok(in_memory_binary_cache) = in_memory_binary_cache.lock() {
                if let Some(cached) = in_memory_binary_cache.get(&id) {
                    return Ok(Cow::Owned(cached.clone()));
                }
            }
        }
        let resolved = self.file_resolver.resolve_binary(id)?;
        if let Some(in_memory_binary_cache) = in_memory_binary_cache {
            if let Ok(mut in_memory_binary_cache) = in_memory_binary_cache.lock() {
                in_memory_binary_cache.insert(id, resolved.as_ref().clone());
            }
        }
        Ok(resolved)
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<Source>> {
        let Self {
            in_memory_source_cache,
            ..
        } = self;

        if let Some(in_memory_source_cache) = in_memory_source_cache {
            if let Ok(in_memory_source_cache) = in_memory_source_cache.lock() {
                if let Some(cached) = in_memory_source_cache.get(&id) {
                    return Ok(Cow::Owned(cached.clone()));
                }
            }
        }
        let resolved = self.file_resolver.resolve_source(id)?;
        if let Some(in_memory_source_cache) = in_memory_source_cache {
            if let Ok(mut in_memory_source_cache) = in_memory_source_cache.lock() {
                in_memory_source_cache.insert(id, resolved.as_ref().clone());
            }
        }
        Ok(resolved)
    }
}
