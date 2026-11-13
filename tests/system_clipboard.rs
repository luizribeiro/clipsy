//! Uses the real clipboard, so it only runs with CLIPSY_SYSTEM_CLIPBOARD_TESTS=1
//! (on Linux, under `xvfb-run` to keep your session's clipboard out of it).
use clipsy::clipboard::{Clipboard, SystemClipboard};

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
    let clipboard = SystemClipboard::default();

    for i in 0..1000 {
        let content = format!("yank {i}\nwith ünïcode");
        clipboard.write(content.clone()).unwrap();
        assert_eq!(clipboard.read().unwrap(), content);
    }
}
