use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    external_term::{self, Limits},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Classic,
    Incremental,
}
impl Kind {
    fn tag(self) -> &'static str {
        match self {
            Self::Classic => "file_plt",
            Self::Incremental => "ifile_plt",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    NoSuchFile,
    ReadError,
    NotValid,
    UnsupportedTerm(u8),
    ResourceLimit,
}
impl From<external_term::Error> for ReadError {
    fn from(error: external_term::Error) -> Self {
        match error {
            external_term::Error::UnsupportedTag(tag) => Self::UnsupportedTerm(tag),
            external_term::Error::ResourceLimit => Self::ResourceLimit,
            external_term::Error::Invalid | external_term::Error::Compression => Self::NotValid,
        }
    }
}

#[derive(Clone)]
pub struct TermHandle {
    arena: Arc<ErlangTerms>,
    root: TermId,
}
impl TermHandle {
    pub fn raw(&self) -> crate::native_form_environment::RawFormTerm {
        crate::native_form_environment::RawFormTerm::new(self.arena.clone(), self.root)
            .expect("snapshot field handle")
    }
    pub fn arena(&self) -> &ErlangTerms {
        &self.arena
    }
    pub fn root(&self) -> TermId {
        self.root
    }
    pub fn term(&self) -> &ErlangTerm {
        &self.arena.terms[self.root.0]
    }
    pub fn decode_binary(&self, limits: Limits) -> Result<Self, ReadError> {
        let ErlangTerm::Binary(bytes) = self.term() else {
            return Err(ReadError::NotValid);
        };
        let arena = Arc::new(external_term::decode(bytes, limits)?);
        Ok(Self {
            root: arena.root,
            arena,
        })
    }
}

#[derive(Clone, Copy)]
pub enum Field {
    Version,
    Fingerprints,
    Info,
    Contracts,
    Callbacks,
    Types,
    ExportedTypes,
    DependenciesOrIncrementalData,
    ImplementationFingerprints,
}
impl Field {
    fn index(self) -> usize {
        match self {
            Self::Version => 1,
            Self::Fingerprints => 2,
            Self::Info => 3,
            Self::Contracts => 4,
            Self::Callbacks => 5,
            Self::Types => 6,
            Self::ExportedTypes => 7,
            Self::DependenciesOrIncrementalData => 8,
            Self::ImplementationFingerprints => 9,
        }
    }
}

#[derive(Clone)]
pub struct SnapshotFile {
    kind: Kind,
    arena: Arc<ErlangTerms>,
    fields: [TermId; 10],
}
impl SnapshotFile {
    pub fn from_bytes(kind: Kind, bytes: &[u8], limits: Limits) -> Result<Self, ReadError> {
        let arena = Arc::new(external_term::decode(bytes, limits)?);
        let ErlangTerm::Tuple(fields) = &arena.terms[arena.root.0] else {
            return Err(ReadError::NotValid);
        };
        let fields: [TermId; 10] = fields
            .as_slice()
            .try_into()
            .map_err(|_| ReadError::NotValid)?;
        if !matches!(&arena.terms[fields[0].0], ErlangTerm::Atom(tag) if tag == kind.tag()) {
            return Err(ReadError::NotValid);
        }
        Ok(Self {
            kind,
            arena,
            fields,
        })
    }
    pub fn read(kind: Kind, path: &Path, limits: Limits) -> Result<Self, ReadError> {
        let mut file = File::open(path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => ReadError::NoSuchFile,
            _ => ReadError::ReadError,
        })?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(limits.bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| ReadError::ReadError)?;
        if bytes.len() > limits.bytes {
            return Err(ReadError::ResourceLimit);
        }
        Self::from_bytes(kind, &bytes, limits)
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn arena(&self) -> &ErlangTerms {
        &self.arena
    }
    pub fn field(&self, field: Field) -> TermHandle {
        TermHandle {
            arena: self.arena.clone(),
            root: self.fields[field.index()],
        }
    }
    pub fn incremental_data(&self, limits: Limits) -> Result<TermHandle, ReadError> {
        if self.kind != Kind::Incremental {
            return Err(ReadError::NotValid);
        }
        let value = self.field(Field::DependenciesOrIncrementalData);
        match value.term() {
            ErlangTerm::Binary(_) => value.decode_binary(limits),
            ErlangTerm::Tuple(fields)
                if fields.len() == 4
                    && matches!(
                        &value.arena.terms[fields[0].0], ErlangTerm::Atom(tag) if tag == "incremental_data"
                    ) =>
            {
                Ok(value)
            }
            _ => Err(ReadError::NotValid),
        }
    }
    pub fn encoded(&self, limits: Limits) -> Result<Vec<u8>, external_term::Error> {
        external_term::encode(&self.arena, limits, self.kind == Kind::Classic)
    }
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct PendingFile(PathBuf);
impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let resolved;
    let path = if fs::symlink_metadata(path).is_ok() {
        OpenOptions::new().write(true).open(path)?;
        resolved = fs::canonicalize(path)?;
        resolved.as_path()
    } else {
        path
    };
    let filename = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "snapshot filename"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let old_permissions = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let (mut file, pending) = loop {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let mut name = filename.to_os_string();
        name.push(format!(".tmp.{}.{}", std::process::id(), serial));
        let candidate = parent.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (file, PendingFile(candidate)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    if let Some(permissions) = old_permissions {
        file.set_permissions(permissions)?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&pending.0, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readers_observe_complete_snapshots_during_repeated_replacement() {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("snapshot-{}-{serial}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("cache");
        let a = vec![0; 65536];
        let b = vec![1; 65536];
        atomic_write(&path, &a).unwrap();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        std::thread::scope(|scope| {
            let reader_done = done.clone();
            let path = &path;
            let a = &a;
            let b = &b;
            scope.spawn(move || {
                while !reader_done.load(Ordering::Acquire) {
                    let actual = fs::read(path).unwrap();
                    assert!(actual == *a || actual == *b);
                }
            });
            for i in 0..100 {
                atomic_write(path, if i % 2 == 0 { b } else { a }).unwrap();
            }
            done.store(true, Ordering::Release);
        });
        assert_eq!(fs::read(&path).unwrap(), a);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn replacing_a_symlink_preserves_the_link_and_target_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("snapshot-link-{}-{serial}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let target = directory.join("target");
        let link = directory.join("link");
        fs::write(&target, b"old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&target, &link).unwrap();
        atomic_write(&link, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
        fs::remove_dir_all(directory).unwrap();
    }
}
