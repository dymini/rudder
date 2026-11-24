//! Copy a Git work tree onto a server without its `.git` directory.
//!
//! The panel clones the repository. A remote server receives only the tracked
//! files, as a tar extracted under the node data directory. A manifest beside
//! the checkout lists those files so the next copy can remove ones Git deleted.
//! Files that were never in the manifest, such as a bind-mounted `./data`, stay.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::path::{Component, Path};
use std::process::Command;

use rudder_core::Error;

/// Largest checkout copied to a remote server.
pub const MAX_TREE_BYTES: u64 = 512 * 1024 * 1024;

/// Manifest stored next to `stacks/<project>/src`, not inside the work tree.
pub fn manifest_relative(tree: &str) -> String {
    format!("{tree}.manifest")
}

/// Paths Git tracks in `checkout`, relative and sorted. Untracked files are omitted.
pub fn git_tracked(checkout: &Path) -> Result<Vec<String>, Error> {
    if !checkout.join(".git").exists() {
        return Err(Error::Validation("the checkout has no git metadata".into()));
    }
    let output = Command::new("git")
        .args(["ls-files", "-z", "--"])
        .current_dir(checkout)
        .output()
        .map_err(|error| Error::Node(format!("git ls-files: {error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Node(format!(
            "git ls-files failed: {}",
            detail.trim()
        )));
    }
    let mut files = Vec::new();
    for raw in output.stdout.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(raw)
            .map_err(|_| Error::Validation("git listed a file path that is not UTF-8".into()))?;
        check_relative(Path::new(text))?;
        files.push(text.to_owned());
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Tar of `files`, which are paths relative to `root`. Symlinks are stored as
/// the file they point at, and only when that file stays inside `root`.
pub fn pack_files(root: &Path, files: &[String]) -> Result<Vec<u8>, Error> {
    let root = root
        .canonicalize()
        .map_err(|error| Error::Node(format!("checkout: {error}")))?;
    let mut builder = tar::Builder::new(Vec::new());
    builder.mode(tar::HeaderMode::Deterministic);
    let mut total = 0u64;
    for rel in files {
        check_relative(Path::new(rel))?;
        let full = root.join(rel);
        let canonical = full
            .canonicalize()
            .map_err(|_| Error::Validation(format!("{rel} is not a file in the checkout")))?;
        if !canonical.starts_with(&root) {
            return Err(Error::Validation(format!(
                "{rel} points outside the checkout"
            )));
        }
        if !canonical.is_file() {
            return Err(Error::Validation(format!(
                "{rel} is not a file, so it was not copied"
            )));
        }
        let mut file =
            File::open(&canonical).map_err(|error| Error::Node(format!("read {rel}: {error}")))?;
        let metadata = file
            .metadata()
            .map_err(|error| Error::Node(error.to_string()))?;
        let len = metadata.len();
        total = total.saturating_add(len);
        if total > MAX_TREE_BYTES {
            return Err(Error::Validation(
                "the checkout is larger than 512 MiB, so it was not copied".into(),
            ));
        }
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 != 0 {
                header.set_mode(0o755);
            }
        }
        header.set_size(len);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, rel, &mut file)
            .map_err(|error| Error::Node(format!("pack {rel}: {error}")))?;
    }
    builder
        .finish()
        .map_err(|error| Error::Node(format!("pack checkout: {error}")))?;
    builder
        .into_inner()
        .map_err(|error| Error::Node(format!("pack checkout: {error}")))
}

/// Extract `archive` into `dest`. Files listed in the previous manifest and
/// absent from this archive are removed. Anything else under `dest` is kept.
/// Paths are resolved one component at a time without following symlinks, so a
/// link planted in `dest` (for example from a container that bind-mounts it)
/// cannot redirect a write or delete outside the checkout.
pub fn apply_tree(dest: &Path, manifest: &Path, archive: &[u8]) -> Result<(), Error> {
    if archive.len() as u64 > MAX_TREE_BYTES {
        return Err(Error::Validation(
            "the checkout is larger than 512 MiB, so it was not copied".into(),
        ));
    }
    fs::create_dir_all(dest).map_err(|error| Error::Node(error.to_string()))?;
    let root = safe::open_root(dest)?;
    if let Some(parent) = manifest.parent() {
        fs::create_dir_all(parent).map_err(|error| Error::Node(error.to_string()))?;
    }
    let mut unpacked = tar::Archive::new(Cursor::new(archive));
    let mut written = BTreeSet::new();
    let mut total = 0u64;
    let entries = unpacked
        .entries()
        .map_err(|error| Error::Node(format!("read checkout archive: {error}")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|error| Error::Node(format!("read checkout archive: {error}")))?;
        let rel = entry
            .path()
            .map_err(|error| Error::Node(format!("checkout path: {error}")))?
            .into_owned();
        check_relative(&rel)?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            safe::open_dir(&root, &rel, true)?;
            continue;
        }
        if !kind.is_file() {
            return Err(Error::Validation(format!(
                "the checkout archive contains an unsupported entry {}",
                rel.display()
            )));
        }
        let size = entry.header().size().unwrap_or(0);
        total = total.saturating_add(size);
        if total > MAX_TREE_BYTES {
            return Err(Error::Validation(
                "the checkout is larger than 512 MiB, so it was not copied".into(),
            ));
        }
        let mut file = safe::create_file(&root, &rel)?;
        let copied = std::io::copy(&mut entry, &mut file)
            .map_err(|error| Error::Node(format!("write {}: {error}", rel.display())))?;
        if copied > size {
            return Err(Error::Validation(
                "the checkout archive is larger than it claims".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let executable = entry.header().mode().unwrap_or(0) & 0o111 != 0;
            file.set_permissions(fs::Permissions::from_mode(if executable {
                0o755
            } else {
                0o644
            }))
            .map_err(|error| Error::Node(format!("checkout permissions: {error}")))?;
        }
        written.insert(normalize(&rel));
    }
    for stale in read_manifest(manifest) {
        if !written.contains(&stale) && check_relative(Path::new(&stale)).is_ok() {
            let _ = safe::remove_file(&root, Path::new(&stale));
        }
    }
    write_manifest(manifest, &written)
}

/// Reject absolute paths, `..`, and any `.git` component.
pub fn check_relative(path: &Path) -> Result<(), Error> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(Error::Validation(
            "checkout path must stay inside the repository".into(),
        ));
    }
    let mut parts = 0;
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                if text == ".git" || text.contains('\0') {
                    return Err(Error::Validation(
                        "checkout path must stay inside the repository".into(),
                    ));
                }
                parts += 1;
            }
            Component::CurDir => {}
            _ => {
                return Err(Error::Validation(
                    "checkout path must stay inside the repository".into(),
                ));
            }
        }
    }
    if parts == 0 {
        return Err(Error::Validation(
            "checkout path must stay inside the repository".into(),
        ));
    }
    Ok(())
}

fn normalize(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn read_manifest(path: &Path) -> BTreeSet<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn write_manifest(path: &Path, files: &BTreeSet<String>) -> Result<(), Error> {
    let mut body = files.iter().cloned().collect::<Vec<_>>().join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    let mut file =
        File::create(path).map_err(|error| Error::Node(format!("write manifest: {error}")))?;
    file.write_all(body.as_bytes())
        .map_err(|error| Error::Node(format!("write manifest: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| Error::Node(error.to_string()))?;
    }
    Ok(())
}

/// Path operations confined to a checkout directory.
#[cfg(unix)]
mod safe {
    use std::fs::File;
    use std::os::fd::{AsFd, OwnedFd};
    use std::path::{Component, Path};

    use rudder_core::Error;
    use rustix::fs::{AtFlags, Mode, OFlags, CWD};
    use rustix::io::Errno;

    const DIR_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    pub(super) struct Root(OwnedFd);

    pub(super) fn open_root(dest: &Path) -> Result<Root, Error> {
        rustix::fs::openat(CWD, dest, DIR_FLAGS, Mode::empty())
            .map(Root)
            .map_err(|errno| refuse(dest, errno))
    }

    fn refuse(rel: &Path, errno: Errno) -> Error {
        if errno == Errno::LOOP || errno == Errno::NOTDIR {
            Error::Validation(format!(
                "{} goes through a symlink or non-directory in the checkout, so it was not written",
                rel.display()
            ))
        } else {
            Error::Node(format!(
                "{}: {}",
                rel.display(),
                std::io::Error::from(errno)
            ))
        }
    }

    fn parts(rel: &Path) -> Vec<&std::ffi::OsStr> {
        rel.components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part),
                _ => None,
            })
            .collect()
    }

    fn step(dir: &OwnedFd, name: &std::ffi::OsStr, create: bool) -> Result<OwnedFd, Errno> {
        match rustix::fs::openat(dir, name, DIR_FLAGS, Mode::empty()) {
            Err(Errno::NOENT) if create => {
                match rustix::fs::mkdirat(dir, name, Mode::from_raw_mode(0o755)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(errno) => return Err(errno),
                }
                rustix::fs::openat(dir, name, DIR_FLAGS, Mode::empty())
            }
            other => other,
        }
    }

    fn walk(
        root: &Root,
        rel: &Path,
        dirs: &[&std::ffi::OsStr],
        create: bool,
    ) -> Result<Option<OwnedFd>, Error> {
        let mut current = root
            .0
            .as_fd()
            .try_clone_to_owned()
            .map_err(|error| Error::Node(error.to_string()))?;
        for name in dirs {
            current = match step(&current, name, create) {
                Ok(next) => next,
                Err(Errno::NOENT) if !create => return Ok(None),
                Err(errno) => return Err(refuse(rel, errno)),
            };
        }
        Ok(Some(current))
    }

    pub(super) fn open_dir(
        root: &Root,
        rel: &Path,
        create: bool,
    ) -> Result<Option<OwnedFd>, Error> {
        walk(root, rel, &parts(rel), create)
    }

    pub(super) fn create_file(root: &Root, rel: &Path) -> Result<File, Error> {
        let parts = parts(rel);
        let Some((name, dirs)) = parts.split_last() else {
            return Err(Error::Validation(
                "checkout path must stay inside the repository".into(),
            ));
        };
        let parent = walk(root, rel, dirs, true)?
            .ok_or_else(|| Error::Node(format!("{}: missing directory", rel.display())))?;
        let fd = rustix::fs::openat(
            &parent,
            *name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o644),
        )
        .map_err(|errno| refuse(rel, errno))?;
        rustix::fs::fchmod(&fd, Mode::from_raw_mode(0o644)).map_err(|errno| refuse(rel, errno))?;
        Ok(File::from(fd))
    }

    pub(super) fn remove_file(root: &Root, rel: &Path) -> Result<(), Error> {
        let parts = parts(rel);
        let Some((name, dirs)) = parts.split_last() else {
            return Ok(());
        };
        let Some(parent) = walk(root, rel, dirs, false)? else {
            return Ok(());
        };
        rustix::fs::unlinkat(&parent, *name, AtFlags::empty()).map_err(|errno| refuse(rel, errno))
    }
}

/// Path operations confined to a checkout directory.
#[cfg(not(unix))]
mod safe {
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};

    use rudder_core::Error;

    pub(super) struct Root(PathBuf);

    pub(super) fn open_root(dest: &Path) -> Result<Root, Error> {
        let meta = fs::symlink_metadata(dest).map_err(|error| Error::Node(error.to_string()))?;
        if !meta.is_dir() {
            return Err(refuse(dest));
        }
        Ok(Root(dest.to_path_buf()))
    }

    fn refuse(rel: &Path) -> Error {
        Error::Validation(format!(
            "{} goes through a symlink or non-directory in the checkout, so it was not written",
            rel.display()
        ))
    }

    fn walk(root: &Root, rel: &Path, create: bool) -> Result<Option<PathBuf>, Error> {
        let mut current = root.0.clone();
        for part in rel.iter() {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(refuse(rel)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if !create {
                        return Ok(None);
                    }
                    fs::create_dir(&current).map_err(|error| Error::Node(error.to_string()))?;
                }
                Err(error) => return Err(Error::Node(error.to_string())),
            }
        }
        Ok(Some(current))
    }

    pub(super) fn open_dir(
        root: &Root,
        rel: &Path,
        create: bool,
    ) -> Result<Option<PathBuf>, Error> {
        walk(root, rel, create)
    }

    pub(super) fn create_file(root: &Root, rel: &Path) -> Result<File, Error> {
        let (Some(name), parent) = (rel.file_name(), rel.parent().unwrap_or(Path::new(""))) else {
            return Err(refuse(rel));
        };
        let dir = walk(root, parent, true)?.ok_or_else(|| refuse(rel))?;
        let target = dir.join(name);
        if fs::symlink_metadata(&target).is_ok_and(|meta| !meta.is_file()) {
            return Err(refuse(rel));
        }
        File::create(&target)
            .map_err(|error| Error::Node(format!("write {}: {error}", rel.display())))
    }

    pub(super) fn remove_file(root: &Root, rel: &Path) -> Result<(), Error> {
        let (Some(name), parent) = (rel.file_name(), rel.parent().unwrap_or(Path::new(""))) else {
            return Ok(());
        };
        let Some(dir) = walk(root, parent, false)? else {
            return Ok(());
        };
        fs::remove_file(dir.join(name)).map_err(|error| Error::Node(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            let path =
                std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> TempDir {
        TempDir::new(name)
    }

    #[test]
    fn copy_keeps_untracked_files_and_drops_deleted_tracked_ones() {
        let dir = scratch("rudder-tree");
        let src = dir.0.join("src");
        fs::create_dir_all(src.join("app")).unwrap();
        fs::write(src.join("app/main.txt"), "one").unwrap();
        fs::write(src.join("README"), "hi").unwrap();
        let packed = pack_files(&src, &["README".into(), "app/main.txt".into()]).unwrap();

        let dest = dir.0.join("remote/src");
        let manifest = dir.0.join("remote/src.manifest");
        fs::create_dir_all(&dest).unwrap();
        fs::create_dir_all(dest.join("data")).unwrap();
        fs::write(dest.join("data/db"), "keep-me").unwrap();
        fs::write(dest.join("old.txt"), "gone").unwrap();
        fs::write(&manifest, "old.txt\nREADME\n").unwrap();

        apply_tree(&dest, &manifest, &packed).unwrap();
        assert_eq!(fs::read_to_string(dest.join("README")).unwrap(), "hi");
        assert_eq!(
            fs::read_to_string(dest.join("app/main.txt")).unwrap(),
            "one"
        );
        assert_eq!(fs::read_to_string(dest.join("data/db")).unwrap(), "keep-me");
        assert!(!dest.join("old.txt").exists());
        let listed = fs::read_to_string(&manifest).unwrap();
        assert!(listed.contains("app/main.txt"));
        assert!(listed.contains("README"));
        assert!(!listed.contains("old.txt"));
        assert!(!listed.contains("data/db"));
    }

    #[test]
    fn archive_cannot_escape_the_checkout() {
        let dir = scratch("rudder-tree-escape");
        let bytes = hostile_ustar("../secret", b"nope");
        let dest = dir.0.join("src");
        let err = apply_tree(&dest, &dir.0.join("src.manifest"), &bytes).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("inside the repository") || message.contains(".."),
            "{message}"
        );
        assert!(!dir.0.join("secret").exists());
        assert!(!dest.join("secret").exists());
    }

    /// A ustar member whose name the `tar` builder refuses to write.
    fn hostile_ustar(name: &str, body: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        let size = format!("{:011o}\0", body.len());
        header[124..136].copy_from_slice(size.as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].copy_from_slice(b"        ");
        header[156] = b'0';
        header[257..262].copy_from_slice(b"ustar");
        header[263..265].copy_from_slice(b"00");
        let sum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        let checksum = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(checksum.as_bytes());
        let mut out = header.to_vec();
        out.extend_from_slice(body);
        out.extend(std::iter::repeat_n(0, 512 - body.len()));
        out.extend(std::iter::repeat_n(0, 1024));
        out
    }

    #[test]
    #[cfg(unix)]
    fn symlink_outside_the_checkout_is_not_packed() {
        let dir = scratch("rudder-tree-link");
        let src = dir.0.join("src");
        fs::create_dir_all(&src).unwrap();
        let outside = dir.0.join("secret");
        fs::write(&outside, "hidden").unwrap();
        std::os::unix::fs::symlink(&outside, src.join("leak")).unwrap();
        let err = pack_files(&src, &["leak".into()]).unwrap_err();
        assert!(err.to_string().contains("outside"));
    }

    #[test]
    #[cfg(unix)]
    fn writes_do_not_follow_symlinks_planted_in_the_checkout() {
        let dir = scratch("rudder-tree-planted");
        let src = dir.0.join("src");
        fs::create_dir_all(src.join("config")).unwrap();
        fs::write(src.join("config/job"), "payload").unwrap();
        fs::write(src.join("top"), "payload").unwrap();
        let packed_dir = pack_files(&src, &["config/job".into()]).unwrap();
        let packed_file = pack_files(&src, &["top".into()]).unwrap();

        let outside = dir.0.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("victim"), "original").unwrap();
        let dest = dir.0.join("remote/src");
        let manifest = dir.0.join("remote/src.manifest");
        fs::create_dir_all(&dest).unwrap();
        std::os::unix::fs::symlink(&outside, dest.join("config")).unwrap();
        std::os::unix::fs::symlink(outside.join("victim"), dest.join("top")).unwrap();

        let err = apply_tree(&dest, &manifest, &packed_dir).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        assert!(!outside.join("job").exists());

        let err = apply_tree(&dest, &manifest, &packed_file).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        assert_eq!(
            fs::read_to_string(outside.join("victim")).unwrap(),
            "original"
        );
    }

    #[test]
    #[cfg(unix)]
    fn stale_cleanup_does_not_follow_symlinks_planted_in_the_checkout() {
        let dir = scratch("rudder-tree-stale-link");
        let src = dir.0.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("README"), "hi").unwrap();
        let packed = pack_files(&src, &["README".into()]).unwrap();

        let outside = dir.0.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("victim"), "keep").unwrap();
        let dest = dir.0.join("remote/src");
        let manifest = dir.0.join("remote/src.manifest");
        fs::create_dir_all(&dest).unwrap();
        std::os::unix::fs::symlink(&outside, dest.join("old")).unwrap();
        fs::write(&manifest, "old/victim\n").unwrap();

        apply_tree(&dest, &manifest, &packed).unwrap();
        assert_eq!(fs::read_to_string(outside.join("victim")).unwrap(), "keep");
        assert_eq!(fs::read_to_string(dest.join("README")).unwrap(), "hi");
    }

    #[test]
    fn git_lists_tracked_files_and_skips_untracked_ones() {
        let dir = scratch("rudder-tree-git");
        let repo = &dir.0;
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(repo)
                .status()
                .unwrap();
            assert!(status.success(), "{args:?}");
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        fs::create_dir_all(repo.join("web")).unwrap();
        fs::write(repo.join("web/index.html"), "<p>").unwrap();
        fs::write(repo.join("compose.yml"), "services: {}\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "init"]);
        fs::write(repo.join("data.sqlite"), "local").unwrap();
        let files = git_tracked(repo).unwrap();
        assert_eq!(
            files,
            ["compose.yml".to_owned(), "web/index.html".to_owned()]
        );
    }

    #[test]
    #[cfg(unix)]
    fn executable_build_scripts_survive_tree_transfer_without_special_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("rudder-tree-executable");
        let src = dir.0.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("build.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(src.join("build.sh"), fs::Permissions::from_mode(0o4755)).unwrap();
        let archive = pack_files(&src, &["build.sh".into()]).unwrap();
        let dest = dir.0.join("dest");
        apply_tree(&dest, &dir.0.join("manifest"), &archive).unwrap();
        assert_eq!(
            fs::metadata(dest.join("build.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o755
        );
    }
}
