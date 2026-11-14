use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const SSH: (&str, &str) = ("SSH_CONNECTION", "10.0.0.2 52144 10.0.0.5 22");

fn plugin_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn path_with(dir: &Path) -> String {
    format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn nvim_sets_clipboard(vars: &[(&str, &str)]) -> bool {
    let home = TempDir::new().unwrap();
    let check =
        "call writefile([exists('g:clipboard') ? g:clipboard.name : 'none'], '/dev/stdout')";
    let output = Command::new("nvim")
        .args(["--headless", "--clean", "--cmd"])
        .arg(format!("set rtp^={}", plugin_root().display()))
        .args(["-c", "runtime plugin/clipsy.vim", "-c", check, "-c", "qa!"])
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(vars.iter().copied())
        .output()
        .expect("running nvim");
    assert!(output.status.success(), "{output:?}");
    match String::from_utf8_lossy(&output.stdout).trim() {
        "clipsy" => true,
        "none" => false,
        other => panic!("unexpected g:clipboard: {other}"),
    }
}

fn stub(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn tmux_copy_sends(vars: &[(&str, &str)]) -> bool {
    let bin = TempDir::new().unwrap();
    let sent = bin.path().join("sent");
    stub(bin.path(), "clipsy", &format!("cat > {}", sent.display()));
    let mut child = Command::new(plugin_root().join("scripts/clipsy-copy"))
        .env_clear()
        .env("PATH", path_with(bin.path()))
        .envs(vars.iter().copied())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), b"yanked").unwrap();
    assert!(child.wait().unwrap().success());
    match std::fs::read_to_string(&sent) {
        Ok(content) => {
            assert_eq!(content, "yanked");
            true
        }
        Err(_) => false,
    }
}

#[test]
fn tmux_plugin_points_copy_command_at_the_wrapper() {
    let bin = TempDir::new().unwrap();
    let calls = bin.path().join("calls");
    stub(
        bin.path(),
        "tmux",
        &format!("printf '%s\\n' \"$@\" > {}", calls.display()),
    );

    let status = Command::new("bash")
        .arg(plugin_root().join("clipsy.tmux"))
        .env_clear()
        .env("PATH", path_with(bin.path()))
        .status()
        .unwrap();

    assert!(status.success());
    let wrapper = plugin_root().join("scripts/clipsy-copy");
    assert_eq!(
        std::fs::read_to_string(&calls).unwrap(),
        format!("set\n-s\ncopy-command\n{}\n", wrapper.display())
    );
}

fn tmux(socket_dir: &Path, path: &str, args: &[&str]) {
    let status = Command::new("tmux")
        .args(["-f", "/dev/null"])
        .args(args)
        .env_clear()
        .env("TMUX_TMPDIR", socket_dir)
        .env("PATH", path)
        .status()
        .unwrap();
    assert!(status.success(), "tmux {args:?}");
}

fn copy_line_in(socket_dir: &Path, path: &str, session: &str) {
    for keys in [
        &["copy-mode", "-t", session][..],
        &["send-keys", "-t", session, "-X", "history-top"],
        &["send-keys", "-t", session, "-X", "select-line"],
        &["send-keys", "-t", session, "-X", "copy-pipe-and-cancel"],
    ] {
        tmux(socket_dir, path, keys);
    }
}

/// Some Nix build sandboxes don't mount devpts, so tmux can't open a pane.
fn ptys_available() -> bool {
    !cfg!(target_os = "linux") || Path::new("/dev/pts").is_dir()
}

#[test]
fn tmux_sends_copies_only_from_sessions_attached_over_ssh() {
    if !ptys_available() {
        eprintln!("skipping: tmux needs a pty and /dev/pts is missing");
        return;
    }
    let dir = TempDir::new().unwrap();
    let sent = dir.path().join("sent");
    stub(
        dir.path(),
        "clipsy",
        &format!("cat >> {}; echo >> {}", sent.display(), sent.display()),
    );
    let path = path_with(dir.path());
    let start = |name: &str, ssh: Option<&str>| {
        let mut command = Command::new("tmux");
        command
            .args([
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-s",
                name,
                "-x",
                "80",
                "-y",
                "10",
            ])
            .arg(format!("echo copied-from-{name}; sleep 30"))
            .env_clear()
            .env("TMUX_TMPDIR", dir.path())
            .env("PATH", &path);
        if let Some(ssh) = ssh {
            command.env("SSH_CONNECTION", ssh);
        }
        assert!(command.status().unwrap().success());
    };

    start("local", None);
    let loaded = Command::new("bash")
        .arg(plugin_root().join("clipsy.tmux"))
        .env_clear()
        .env("TMUX_TMPDIR", dir.path())
        .env("PATH", &path)
        .status()
        .unwrap();
    assert!(loaded.success());
    start("remote", Some(SSH.1));
    std::thread::sleep(std::time::Duration::from_millis(300));
    copy_line_in(dir.path(), &path, "local");
    copy_line_in(dir.path(), &path, "remote");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !sent.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    tmux(dir.path(), &path, &["kill-server"]);

    assert_eq!(
        std::fs::read_to_string(&sent).unwrap_or_default().trim(),
        "copied-from-remote"
    );
}

#[test]
fn plugins_turn_on_inside_ssh_sessions() {
    assert!(nvim_sets_clipboard(&[SSH]));
    assert!(tmux_copy_sends(&[SSH]));
}

#[test]
fn plugins_turn_on_when_a_server_is_chosen() {
    assert!(nvim_sets_clipboard(&[("CLIPSY_SERVER", "laptop")]));
    assert!(tmux_copy_sends(&[("CLIPSY_SERVER", "laptop")]));
}

#[test]
fn plugins_stay_off_on_the_machine_running_the_server() {
    assert!(!nvim_sets_clipboard(&[]));
    assert!(!tmux_copy_sends(&[]));
}

#[test]
fn clipsy_enabled_overrides_detection() {
    assert!(nvim_sets_clipboard(&[("CLIPSY_ENABLED", "1")]));
    assert!(tmux_copy_sends(&[("CLIPSY_ENABLED", "1")]));
    assert!(!nvim_sets_clipboard(&[SSH, ("CLIPSY_ENABLED", "0")]));
    assert!(!tmux_copy_sends(&[SSH, ("CLIPSY_ENABLED", "0")]));
}
