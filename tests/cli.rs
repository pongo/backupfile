use std::{
    ffi::OsStr,
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(directory: &Path, args: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_backupfile"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn decode(path: &Path) -> Vec<u8> {
    zstd::stream::decode_all(fs::File::open(path).unwrap()).unwrap()
}

#[test]
fn round_trip_relative_unicode_and_empty_files() {
    let dir = tempfile::tempdir().unwrap();
    for (name, contents) in [
        ("file.txt", b"hello backup".as_slice()),
        ("empty", b"".as_slice()),
        (
            "with spaces \u{6587}\u{4ef6}.txt",
            b"unicode path".as_slice(),
        ),
    ] {
        fs::write(dir.path().join(name), contents).unwrap();
        let output = run(dir.path(), &[OsStr::new(name)]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Backup saved to:"));
        assert_eq!(decode(&dir.path().join(format!("{name}.zst"))), contents);
        assert_eq!(fs::read(dir.path().join(name)).unwrap(), contents);
    }
}

#[test]
fn rejects_invalid_arguments_and_inputs() {
    let dir = tempfile::tempdir().unwrap();
    for args in [vec![], vec![OsStr::new("one"), OsStr::new("two")]] {
        let output = run(dir.path(), &args);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
    for name in ["missing", "."] {
        let output = run(dir.path(), &[OsStr::new(name)]);
        assert_eq!(output.status.code(), Some(1));
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn rejects_directory_at_backup_destination() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file"), b"data").unwrap();
    fs::create_dir(dir.path().join("file.zst")).unwrap();
    let output = run(dir.path(), &[OsStr::new("file")]);
    assert_eq!(output.status.code(), Some(1));
    assert!(dir.path().join("file.zst").is_dir());
}

#[test]
fn repeated_backup_rotates_previous_archive_through_trash() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("rotation-test.txt");
    fs::write(&source, b"original").unwrap();
    assert!(run(dir.path(), &[source.as_os_str()]).status.success());
    fs::write(&source, b"updated").unwrap();
    let output = run(dir.path(), &[source.as_os_str()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        decode(&dir.path().join("rotation-test.txt.zst")),
        b"updated"
    );
    assert_eq!(fs::read(source).unwrap(), b"updated");
}
