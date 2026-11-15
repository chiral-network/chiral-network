//! Release process locks at scope exit, even while a forked child still owns an
//! inherited descriptor before exec. Closing only our descriptor is insufficient.
use fs2::FileExt;
use std::{fs::File, io};

pub(crate) struct ExclusiveFileLock(File);

impl ExclusiveFileLock {
    pub(crate) fn try_acquire(file: File) -> io::Result<Self> {
        FileExt::try_lock_exclusive(&file)?;
        Ok(Self(file))
    }

    pub(crate) fn acquire(file: File) -> io::Result<Self> {
        FileExt::lock_exclusive(&file)?;
        Ok(Self(file))
    }
}

impl Drop for ExclusiveFileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs::OpenOptions;

    #[test]
    fn exclusion_ends_before_inherited_descriptor_closes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("process.lock");
        let open = || {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .unwrap()
        };
        let original = open();
        // dup and fork retain the same open file description on Unix.
        let inherited = original.try_clone().unwrap();
        let owner = ExclusiveFileLock::try_acquire(original).unwrap();
        assert_eq!(
            ExclusiveFileLock::try_acquire(open()).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(owner);
        let next_owner = ExclusiveFileLock::try_acquire(open()).unwrap();
        drop(inherited);
        assert_eq!(
            ExclusiveFileLock::try_acquire(open()).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(next_owner);
        assert!(ExclusiveFileLock::acquire(open()).is_ok());
    }
}
