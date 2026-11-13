//! Uses the real clipboard, so it only runs with CLIPSY_SYSTEM_CLIPBOARD_TESTS=1
//! (on Linux, under `xvfb-run` to keep your session's clipboard out of it).
use clipsy::clipboard::{Clipboard, Entry, SystemClipboard};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// One clipboard for the whole process, as `clipsy serve` keeps: arboard
/// can't open a new X11 clipboard while it's still tearing down the last.
fn exclusive_clipboard() -> MutexGuard<'static, SystemClipboard> {
    static REAL_CLIPBOARD: OnceLock<Mutex<SystemClipboard>> = OnceLock::new();
    REAL_CLIPBOARD
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn enabled() -> bool {
    let enabled = std::env::var_os("CLIPSY_SYSTEM_CLIPBOARD_TESTS").is_some_and(|v| v == "1");
    if !enabled {
        eprintln!("skipping: set CLIPSY_SYSTEM_CLIPBOARD_TESTS=1 to use the real clipboard");
    }
    enabled
}

#[test]
fn many_writes_reuse_one_clipboard_connection() {
    if !enabled() {
        return;
    }
    let clipboard = exclusive_clipboard();

    for i in 0..1000 {
        let content = format!("yank {i}\nwith ünïcode");
        clipboard.write(content.clone().into()).unwrap();
        assert_eq!(clipboard.read().unwrap().text, content);
    }
}

#[test]
fn concealed_entries_are_detected_on_the_real_clipboard() {
    if !enabled() {
        return;
    }
    let clipboard = exclusive_clipboard();

    clipboard.write(Entry::concealed("hunter2")).unwrap();
    assert_eq!(clipboard.read().unwrap(), Entry::concealed("hunter2"));

    clipboard.write(Entry::plain("ls -la")).unwrap();
    assert_eq!(clipboard.read().unwrap(), Entry::plain("ls -la"));
}
