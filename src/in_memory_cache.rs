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

struct Cached<T> {
    file_resolver: T,
    in_memory_source_cache: Option<Arc<Mutex<HashMap<FileId, Source>>>>,
    in_memory_binary_cache: Option<Arc<Mutex<HashMap<FileId, Bytes>>>>,
}

impl<T> FileResolver for Cached<T>
where
    T: FileResolver,
{
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<Bytes>> {
        let Self {
            in_memory_binary_cache,
            ..
        } = self;

        if let Some(in_memory_binary_cache) = in_memory_binary_cache {
            if let Ok(cache) = in_memory_binary_cache.lock() {
                if let Some(cached) = cache.get(&id) {
                    return Ok(Cow::Owned(cached.clone()));
                }
            }
        }
        let resolved = self.file_resolver.resolve_binary(id)?;
        if let Some(in_memory_binary_cache) = self.in_memory_binary_cache {
            let resolved = in_memory_binary_cache
                .borrow_mut()
                .entry(id)
                .or_insert(resolved.into_owned());
            return Ok(Cow::Borrowed(resolved));
        }
        Ok(resolved)
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<Source>> {
        let Self {
            file_resolver,
            in_memory_source_cache,
            ..
        } = self;
        self.file_resolver.resolve_source(id)
    }
}
