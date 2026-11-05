use anyhow::{bail, Context, Result};
use data_encoding::HEXLOWER;
use iroh::SecretKey;
use std::fs;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;
use tempfile::NamedTempFile;

pub fn load_or_create(path: &Path) -> Result<SecretKey> {
    match fs::metadata(path) {
        Ok(_) => load(path),
        Err(e) if e.kind() == ErrorKind::NotFound => create(path),
        Err(e) => Err(e).with_context(|| format!("checking {}", path.display())),
    }
}

fn load(path: &Path) -> Result<SecretKey> {
    let mode = fs::metadata(path)
        .with_context(|| format!("checking {}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        bail!(
            "{} is accessible by other users (mode {:o}); run `chmod 600` on it",
            path.display(),
            mode & 0o777
        );
    }
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.trim()
        .parse()
        .with_context(|| format!("{} does not hold a valid secret key", path.display()))
}

fn create(path: &Path) -> Result<SecretKey> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    create_private_dir(dir)?;
    let key = SecretKey::generate();
    let mut staging = NamedTempFile::new_in(dir)
        .with_context(|| format!("creating a temporary key in {}", dir.display()))?;
    writeln!(staging, "{}", HEXLOWER.encode(&key.to_bytes()))?;
    staging.as_file().sync_all()?;
    match staging.persist_noclobber(path) {
        Ok(_) => Ok(key),
        Err(e) if e.error.kind() == ErrorKind::AlreadyExists => load(path),
        Err(e) => Err(e.error).with_context(|| format!("creating {}", path.display())),
    }
}

fn create_private_dir(dir: &Path) -> Result<()> {
    if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(e) if e.kind() != ErrorKind::AlreadyExists => {
            Err(e).with_context(|| format!("creating {}", dir.display()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    const KNOWN_SECRET_HEX: &str =
        "1f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a7988";

    fn known_key() -> SecretKey {
        KNOWN_SECRET_HEX.parse().unwrap()
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn write_key_file(path: &Path, contents: &str, mode: u32) {
        fs::write(path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn first_run_creates_private_key_in_private_dir() {
        let home = TempDir::new().unwrap();
        let path = home.path().join(".local/state/clipsy/secret.key");

        load_or_create(&path).unwrap();

        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_ne!(mode(&home.path().join(".local")), 0o700);
    }

    #[test]
    fn keeps_existing_dir_permissions() {
        let dir = TempDir::new().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();

        load_or_create(&dir.path().join("secret.key")).unwrap();

        assert_eq!(mode(dir.path()), 0o755);
    }

    #[test]
    fn later_runs_keep_the_same_identity() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secret.key");

        let first = load_or_create(&path).unwrap();
        let second = load_or_create(&path).unwrap();

        assert_eq!(first.public(), second.public());
        assert_eq!(entries(dir.path()), ["secret.key"]);
    }

    #[test]
    fn loads_existing_hex_key_with_trailing_newline() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secret.key");
        write_key_file(&path, &format!("{KNOWN_SECRET_HEX}\n"), 0o600);

        assert_eq!(
            load_or_create(&path).unwrap().public(),
            known_key().public()
        );
    }

    #[test]
    fn accepts_owner_only_modes() {
        for allowed in [0o400, 0o600, 0o700] {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("secret.key");
            write_key_file(&path, KNOWN_SECRET_HEX, allowed);

            assert!(load_or_create(&path).is_ok(), "mode {allowed:o}");
        }
    }

    #[test]
    fn refuses_keys_accessible_by_others() {
        for leaky in [0o640, 0o604, 0o644, 0o666, 0o610] {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("secret.key");
            write_key_file(&path, KNOWN_SECRET_HEX, leaky);

            let err = load_or_create(&path).unwrap_err();

            assert!(
                err.to_string().contains("chmod 600"),
                "mode {leaky:o}: {err}"
            );
        }
    }

    #[test]
    fn refuses_corrupted_keys_instead_of_replacing_them() {
        let wrong_length = &KNOWN_SECRET_HEX[..62];
        let not_hex = "zz".repeat(32);
        for corrupted in ["", "not a key", wrong_length, &not_hex] {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("secret.key");
            write_key_file(&path, corrupted, 0o600);

            let err = load_or_create(&path).unwrap_err();

            assert!(
                err.to_string().contains("secret.key"),
                "{corrupted:?}: {err}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), corrupted);
        }
    }

    #[test]
    fn follows_symlink_to_rendered_secret() {
        let dir = TempDir::new().unwrap();
        let rendered = dir.path().join("rendered-secret");
        write_key_file(&rendered, KNOWN_SECRET_HEX, 0o400);
        let link = dir.path().join("secret.key");
        symlink(&rendered, &link).unwrap();

        assert_eq!(
            load_or_create(&link).unwrap().public(),
            known_key().public()
        );
    }

    #[test]
    fn dangling_symlink_is_an_error_and_left_alone() {
        let dir = TempDir::new().unwrap();
        let link = dir.path().join("secret.key");
        symlink(dir.path().join("not-rendered-yet"), &link).unwrap();

        assert!(load_or_create(&link).is_err());
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(entries(dir.path()), ["secret.key"]);
    }

    #[test]
    fn create_returns_existing_key_instead_of_overwriting() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secret.key");
        write_key_file(&path, KNOWN_SECRET_HEX, 0o600);

        assert_eq!(create(&path).unwrap().public(), known_key().public());
        assert_eq!(fs::read_to_string(&path).unwrap(), KNOWN_SECRET_HEX);
        assert_eq!(entries(dir.path()), ["secret.key"]);
    }

    #[test]
    fn concurrent_first_runs_agree_on_one_identity() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secret.key");

        let ids: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| load_or_create(&path).unwrap().public()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        assert!(ids.windows(2).all(|pair| pair[0] == pair[1]));
        assert_eq!(entries(dir.path()), ["secret.key"]);
    }

    #[test]
    fn parent_that_is_a_file_is_an_error() {
        let dir = TempDir::new().unwrap();
        let blocker = dir.path().join("clipsy");
        fs::write(&blocker, "").unwrap();

        assert!(load_or_create(&blocker.join("secret.key")).is_err());
        assert_eq!(entries(dir.path()), ["clipsy"]);
    }

    #[test]
    fn read_only_dir_is_an_error_without_leftovers() {
        let dir = TempDir::new().unwrap();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();

        let result = load_or_create(&locked.join("secret.key"));

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert!(entries(&locked).is_empty());
    }

    #[test]
    fn bare_relative_path_is_created_in_current_dir() {
        let dir = TempDir::new().unwrap();
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = load_or_create(Path::new("secret.key"));

        std::env::set_current_dir(previous).unwrap();
        result.unwrap();
        assert_eq!(entries(dir.path()), ["secret.key"]);
    }
}
