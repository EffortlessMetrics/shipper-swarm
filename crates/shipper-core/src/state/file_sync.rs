//! The private file-sync seam shared by event and JSON persistence.
//!
//! Fault observation exists only in unit-test builds, is scoped to one temporary
//! directory, and runs after the caller has written/flushed the actual file.

use std::fs::File;
use std::io;
use std::path::Path;

pub(super) fn sync_all(file: &File, _path: &Path) -> io::Result<()> {
    #[cfg(test)]
    test_support::before_sync(_path)?;
    file.sync_all()
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    type Observer = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

    static OBSERVER: Mutex<Option<(PathBuf, Observer)>> = Mutex::new(None);

    pub(crate) struct Guard;

    /// Serial tests install one observer for their own absolute temporary root.
    /// Other tests and worker threads writing elsewhere are unaffected.
    pub(crate) fn observe(
        root: &Path,
        observer: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static,
    ) -> Guard {
        assert!(root.is_absolute());
        let mut active = OBSERVER.lock().expect("file-sync observer mutex");
        assert!(active.is_none(), "file-sync tests must run serially");
        *active = Some((root.to_path_buf(), Arc::new(observer)));
        Guard
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            *OBSERVER.lock().expect("file-sync observer mutex") = None;
        }
    }

    pub(super) fn before_sync(path: &Path) -> io::Result<()> {
        let observer = OBSERVER
            .lock()
            .expect("file-sync observer mutex")
            .as_ref()
            .filter(|(root, _)| path.starts_with(root))
            .map(|(_, observer)| Arc::clone(observer));
        if let Some(observer) = observer {
            observer(path)?;
        }
        Ok(())
    }
}
