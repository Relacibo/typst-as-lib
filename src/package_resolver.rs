use std::{borrow::Cow, cell::RefCell, collections::HashMap, io::Read, rc::Rc};

use binstall_tar::Archive;
use ecow::eco_format;
use flate2::read::GzDecoder;
use typst::{
    diag::{FileResult, PackageError},
    foundations::Bytes,
    syntax::{package::PackageSpec, FileId, Source, VirtualPath},
};

use crate::{
    file_resolver::FileResolver,
    util::{bytes_to_source, not_found},
};

static PACKAGE_REPOSITORY_URL: &str = "https://packages.typst.org";

static REQUEST_RETRY_COUNT: u32 = 3;

#[derive(Debug, Clone)]
pub struct PackageResolver {
    ureq: ureq::Agent,
    cache: Rc<RefCell<HashMap<FileId, Vec<u8>>>>,
}

impl PackageResolver {
    pub fn new(cache: Rc<RefCell<HashMap<FileId, Vec<u8>>>>, ureq: Option<ureq::Agent>) -> Self {
        let ureq = ureq.unwrap_or_else(|| ureq::Agent::new());
        Self { ureq, cache }
    }
}

impl PackageResolver {
    fn resolve_bytes<T>(&self, id: FileId) -> FileResult<T>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>,
    {
        let Self { ureq, cache } = self;
        let Some(package) = id.package() else {
            return Err(not_found(id));
        };
        if let Some(res) = cache.as_ref().borrow().get(&id) {
            return SourceOrBytesCreator.try_create(id, res);
        }
        let PackageSpec {
            namespace,
            name,
            version,
        } = package;

        let url = format!(
            "{}/{}/{}-{}.tar.gz",
            PACKAGE_REPOSITORY_URL, namespace, name, version,
        );

        let mut last_error = eco_format!("");
        let mut response = None;
        for _ in 0..REQUEST_RETRY_COUNT {
            let resp = match ureq.get(&url).call() {
                Ok(resp) => resp,
                Err(error) => {
                    last_error = eco_format!("{error}");
                    continue;
                }
            };

            let status = resp.status();
            if status != 200 {
                last_error = eco_format!("response returned unsuccessful status code {status}",);
                continue;
            }
            response = Some(resp);
            break;
        }
        let response = response.ok_or_else(|| PackageError::NetworkFailed(Some(last_error)))?;

        let mut d = GzDecoder::new(response.into_reader());
        let mut archive = Vec::new();
        d.read_to_end(&mut archive)
            .map_err(|error| PackageError::MalformedArchive(Some(eco_format!("{error}"))))?;

        let mut archive = Archive::new(&archive[..]);
        let entries = archive
            .entries()
            .map_err(|error| PackageError::MalformedArchive(Some(eco_format!("{error}"))))?;
        let mut cache = cache.as_ref().borrow_mut();
        for entry in entries {
            let Ok(mut file) = entry else {
                continue;
            };
            let Ok(p) = file.path() else {
                continue;
            };
            let Some(file_name) = p.file_name() else {
                continue;
            };
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            let file_id = FileId::new(Some(package.clone()), VirtualPath::new(file_name));
            let mut buf = Vec::new();
            let Ok(_) = file.read_to_end(&mut buf) else {
                continue;
            };
            cache.insert(file_id, buf);
        }

        let bytes = cache.get(&id).ok_or_else(|| not_found(id))?;
        SourceOrBytesCreator.try_create(id, bytes)
    }
}

impl FileResolver for PackageResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<Bytes>> {
        self.resolve_bytes(id).map(|b| Cow::Owned(b))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<Source>> {
        self.resolve_bytes(id).map(|s| Cow::Owned(s))
    }
}

struct SourceOrBytesCreator;

trait CreateBytesOrSource<T> {
    fn try_create(&self, id: FileId, value: &[u8]) -> FileResult<T>;
}

impl CreateBytesOrSource<Source> for SourceOrBytesCreator {
    fn try_create(&self, id: FileId, value: &[u8]) -> FileResult<Source> {
        let source = bytes_to_source(id, value)?;
        Ok(source)
    }
}

impl CreateBytesOrSource<Bytes> for SourceOrBytesCreator {
    fn try_create(&self, _id: FileId, value: &[u8]) -> FileResult<Bytes> {
        Ok(Bytes::from(value))
    }
}
