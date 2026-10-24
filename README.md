# backupfile

Create a Zstandard (compression level 3) backup of one file. It moves an existing backup to the system trash.

```sh
$ backupfile "path/to/file.txt"
Backup saved to: path/to/file.txt.zst
```

## Build

```sh
cargo build --release
```
