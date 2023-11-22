//! When no file system is used by a WebC then this is used as a placeholder -
//! as the name suggests it always returns file not found.

use std::path::Path;
use std::sync::Mutex;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};

use crate::*;

#[derive(Debug, Default, Clone)]
pub struct EmptyFileSystem {}

#[allow(unused_variables)]
impl FileSystem for EmptyFileSystem {
    fn as_dir(&self) -> Box<dyn Directory + Send + Sync> {
        Box::new(self.clone())
    }
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        // Special-case the root path by returning an empty iterator.
        // An empty file system should still be readable, just not contain
        // any entries.
        if path == Path::new("/") {
            Ok(ReadDir::new(Vec::new()))
        } else {
            Err(FsError::EntryNotFound)
        }
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        Err(FsError::EntryNotFound)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        Err(FsError::EntryNotFound)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Err(FsError::EntryNotFound) })
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        // Special-case the root path by returning an stub value.
        // An empty file system should still be readable, just not contain
        // any entries.
        if path == Path::new("/") {
            Ok(Metadata {
                ft: FileType::new_dir(),
                accessed: 0,
                created: 0,
                modified: 0,
                len: 0,
            })
        } else {
            Err(FsError::EntryNotFound)
        }
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        Err(FsError::EntryNotFound)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        Err(FsError::EntryNotFound)
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(self)
    }
}

impl crate::Directory for EmptyFileSystem {
    fn unique_id(&self) -> usize {
        unimplemented!();
    }

    fn get_child(&self, _name: OsString) -> Result<Descriptor> {
        Err(FsError::EntryNotFound)
    }

    fn iter(&self) -> ReaddirIterator {
        ReaddirIterator(Mutex::new(Box::new(vec![].into_iter())))
    }
    fn absolute_path(&self) -> PathBuf {
        // let guard = self.fs.inner.read().unwrap();
        // let node = guard.get_node(self.inode).unwrap();
        // guard.absolute_path(node)
        PathBuf::from("/")
    }

    fn walk_to<'a>(&self, _to: PathBuf) -> Result<Arc<dyn crate::Directory + Send + Sync>> {
        Err(FsError::EntryNotFound)
    }

    fn parent(&self) -> Option<Arc<dyn crate::Directory + Send + Sync>> {
        unimplemented!();
    }
}

impl FileOpener for EmptyFileSystem {
    #[allow(unused_variables)]
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        Err(FsError::EntryNotFound)
    }
}