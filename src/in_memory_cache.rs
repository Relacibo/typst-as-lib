use std::{
    borrow::Cow,
    cell::RefCell,
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
    file_resolver: T,
    in_memory_source_cache: Option<Arc<Mutex<HashMap<FileId, Source>>>>,
    in_memory_binary_cache: Option<Arc<Mutex<HashMap<FileId, Bytes>>>>,
}

impl<T> CachedFileResolver<T> {
    pub fn new(file_resolver: T) -> Self {
        CachedFileResolver {
            file_resolver,
            in_memory_source_cache: None,
            in_memory_binary_cache: None,
        }
    }

    pub fn set_in_memory_source_cache(
        &mut self,
        in_memory_source_cache: Arc<Mutex<HashMap<FileId, Source>>>,
    ) -> &mut Self {
        self.in_memory_source_cache = Some(in_memory_source_cache);
        self
    }

    pub fn with_in_memory_source_cache(self) -> Self {
        Self {
            in_memory_source_cache: Some(Default::default()),
            ..self
        }
    }

    pub fn in_memory_source_cache(&self) -> &Option<Arc<Mutex<HashMap<FileId, Source>>>> {
        &self.in_memory_source_cache
    }

    pub fn set_in_memory_binary_cache(
        &mut self,
        in_memory_binary_cache: Arc<Mutex<HashMap<FileId, Bytes>>>,
    ) -> &mut Self {
        self.in_memory_binary_cache = Some(in_memory_binary_cache);
        self
    }

    pub fn with_in_memory_binary_cache(self) -> Self {
        Self {
            in_memory_binary_cache: Some(Default::default()),
            ..self
        }
    }

    pub fn in_memory_binary_cache(&self) -> &Option<Arc<Mutex<HashMap<FileId, Bytes>>>> {
        &self.in_memory_binary_cache
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
