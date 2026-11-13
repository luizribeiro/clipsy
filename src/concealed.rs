//! Password managers mark the entries they copy so clipboard history
//! skips them: macOS uses the nspasteboard.org `ConcealedType`, Linux
//! (X11 and Wayland) the KDE `x-kde-passwordManagerHint`.

#[cfg(target_os = "macos")]
pub fn mark(set: arboard::Set<'_>) -> arboard::Set<'_> {
    use arboard::SetExtApple;
    set.exclude_from_history()
}

#[cfg(target_os = "linux")]
pub fn mark(set: arboard::Set<'_>) -> arboard::Set<'_> {
    use arboard::SetExtLinux;
    set.exclude_from_history()
}
