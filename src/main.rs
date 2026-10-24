use std::{
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(path) = args.next() else {
        eprintln!("Usage: backupfile <FILE>");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("Usage: backupfile <FILE>");
        return ExitCode::from(2);
    }

    match backup_file(Path::new(&path)) {
        Ok(backup) => {
            println!("Backup saved to: {}", backup.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("backupfile: {}: {error}", Path::new(&path).display());
            ExitCode::FAILURE
        }
    }
}

fn backup_file(path: &Path) -> Result<PathBuf> {
    let mut input = fs::File::open(path)?;
    if !input.metadata()?.is_file() {
        return Err(io::Error::other("Input must be a regular file").into());
    }

    let mut name = path.as_os_str().to_os_string();
    name.push(".zst");
    let backup = PathBuf::from(name);
    validate_destination(&backup)?;
    let parent = backup
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut compressed = tempfile::NamedTempFile::new_in(parent)?;
    let mut encoder = zstd::stream::write::Encoder::new(compressed.as_file_mut(), 3)?;
    io::copy(&mut input, &mut encoder)?;
    encoder.finish()?;
    compressed.as_file().sync_all()?;

    publish_backup(compressed, &backup, |path| {
        trash::delete(path).map_err(Into::into)
    })?;
    Ok(backup)
}

fn validate_destination(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(io::Error::other(format!(
                "Backup destination must be a regular file: {}",
                path.display()
            ))
            .into())
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn publish_backup(
    compressed: tempfile::NamedTempFile,
    backup: &Path,
    remove: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    if validate_destination(backup)? {
        remove(backup)?;
    }
    // Do not overwrite a destination created between validation and publication.
    if let Err(error) = compressed.persist_noclobber(backup) {
        let reason = error.error;
        match error.file.keep() {
            Ok((_file, recovery)) => {
                return Err(io::Error::other(format!(
                    "Could not save backup to {}: {reason}. Compressed backup retained at: {}",
                    backup.display(),
                    recovery.display()
                ))
                .into());
            }
            Err(mut keep_error) => {
                keep_error.file.disable_cleanup(true);
                return Err(io::Error::other(format!(
                    "Could not save backup to {}: {reason}. Temporary archive left at: {} (could not finalize its attributes: {})",
                    backup.display(), keep_error.file.path().display(), keep_error.error
                )).into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use io::Write;

    #[test]
    fn trash_failure_preserves_previous_backup() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("file.zst");
        fs::write(&backup, b"previous").unwrap();
        let mut compressed = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        compressed.write_all(b"new").unwrap();
        let temporary = compressed.path().to_path_buf();
        let result = publish_backup(compressed, &backup, |_| {
            Err(io::Error::other("Trash failed").into())
        });
        assert!(result.unwrap_err().to_string().contains("Trash failed"));
        assert_eq!(fs::read(backup).unwrap(), b"previous");
        assert!(!temporary.exists());
    }

    #[test]
    fn publication_failure_retains_recovery_file() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("file.zst");
        fs::write(&backup, b"previous").unwrap();
        let mut compressed = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        compressed.write_all(b"new").unwrap();
        let temporary = compressed.path().to_path_buf();
        let result = publish_backup(compressed, &backup, |path| {
            fs::remove_file(path)?;
            fs::write(path, b"concurrent file")?;
            Ok(())
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains(&temporary.display().to_string())
        );
        assert_eq!(fs::read(temporary).unwrap(), b"new");
        assert_eq!(fs::read(backup).unwrap(), b"concurrent file");
    }
}
