//! Password managers mark the entries they copy so clipboard history
//! skips them: macOS uses the nspasteboard.org `ConcealedType`, Linux
//! (X11 and Wayland) the KDE `x-kde-passwordManagerHint`. arboard can set
//! these marks but not read them back, so detection talks to each
//! platform directly.

#[cfg(target_os = "macos")]
pub use macos::{mark, Detector};

#[cfg(target_os = "linux")]
pub use linux::{mark, Detector};

#[cfg(target_os = "macos")]
mod macos {
    use objc2::{msg_send, rc::Retained, ClassType};
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::ns_string;

    pub fn mark(set: arboard::Set<'_>) -> arboard::Set<'_> {
        use arboard::SetExtApple;
        set.exclude_from_history()
    }

    #[derive(Default)]
    pub struct Detector;

    impl Detector {
        pub fn is_concealed(&mut self) -> Result<bool, String> {
            // `generalPasteboard` can return nil under launchd, which the
            // typed binding doesn't allow for.
            unsafe {
                let pasteboard: Option<Retained<NSPasteboard>> =
                    msg_send![NSPasteboard::class(), generalPasteboard];
                let pasteboard = pasteboard.ok_or("the general pasteboard is unavailable")?;
                Ok(pasteboard.types().is_some_and(|types| {
                    types.containsObject(ns_string!("org.nspasteboard.ConcealedType"))
                }))
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashSet;
    use std::error::Error;
    use std::time::{Duration, Instant};
    use wl_clipboard_rs::paste::{self, ClipboardType, Seat};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, CreateWindowAux, WindowClass};
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;
    use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

    const HINT: &str = "x-kde-passwordManagerHint";
    const X11_REPLY_TIMEOUT: Duration = Duration::from_secs(2);

    pub fn mark(set: arboard::Set<'_>) -> arboard::Set<'_> {
        use arboard::SetExtLinux;
        set.exclude_from_history()
    }

    #[derive(Default)]
    pub struct Detector {
        x11: Option<X11Probe>,
    }

    impl Detector {
        pub fn is_concealed(&mut self) -> Result<bool, String> {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                let offered = paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified);
                if let Some(concealed) = classify_wayland(offered)? {
                    return Ok(concealed);
                }
            }
            self.x11_is_concealed()
                .map_err(|e| format!("checking X11 clipboard targets: {e}"))
        }

        fn x11_is_concealed(&mut self) -> Result<bool, Box<dyn Error>> {
            let probe = match &mut self.x11 {
                Some(probe) => probe,
                None => self.x11.insert(X11Probe::connect()?),
            };
            let outcome = probe.is_concealed();
            if outcome.is_err() {
                self.x11 = None;
            }
            outcome
        }
    }

    /// `Ok(None)` means Wayland's data-control protocol is unusable here,
    /// which is exactly when arboard falls back to X11, so we do too.
    pub(super) fn classify_wayland(
        offered: Result<HashSet<String>, paste::Error>,
    ) -> Result<Option<bool>, String> {
        match offered {
            Ok(types) => Ok(Some(types.contains(HINT))),
            Err(paste::Error::ClipboardEmpty | paste::Error::NoSeats) => Ok(Some(false)),
            Err(
                paste::Error::MissingProtocol { .. }
                | paste::Error::SocketOpenError(_)
                | paste::Error::WaylandConnection(_),
            ) => Ok(None),
            Err(e) => Err(format!(
                "checking Wayland clipboard types: {:#}",
                anyhow::Error::new(e)
            )),
        }
    }

    struct X11Probe {
        conn: RustConnection,
        window: u32,
        clipboard: u32,
        targets: u32,
        property: u32,
    }

    impl X11Probe {
        fn connect() -> Result<Self, Box<dyn Error>> {
            let (conn, screen) = x11rb::connect(None)?;
            let intern = |name: &[u8]| -> Result<u32, Box<dyn Error>> {
                Ok(conn.intern_atom(false, name)?.reply()?.atom)
            };
            let clipboard = intern(b"CLIPBOARD")?;
            let targets = intern(b"TARGETS")?;
            let property = intern(b"CLIPSY_TARGETS")?;
            let screen = &conn.setup().roots[screen];
            let window = conn.generate_id()?;
            conn.create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                screen.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                screen.root_visual,
                &CreateWindowAux::new(),
            )?;
            Ok(Self {
                conn,
                window,
                clipboard,
                targets,
                property,
            })
        }

        fn is_concealed(&self) -> Result<bool, Box<dyn Error>> {
            let hint = self.conn.intern_atom(true, HINT.as_bytes())?.reply()?.atom;
            if hint == NONE {
                return Ok(false);
            }
            self.conn.convert_selection(
                self.window,
                self.clipboard,
                self.targets,
                self.property,
                CURRENT_TIME,
            )?;
            self.conn.flush()?;
            let deadline = Instant::now() + X11_REPLY_TIMEOUT;
            loop {
                match self.conn.poll_for_event()? {
                    Some(Event::SelectionNotify(event)) if event.requestor == self.window => {
                        if event.property == NONE {
                            return Ok(false);
                        }
                        let reply = self
                            .conn
                            .get_property(
                                true,
                                self.window,
                                self.property,
                                AtomEnum::ATOM,
                                0,
                                u32::MAX / 4,
                            )?
                            .reply()?;
                        let mut targets = reply
                            .value32()
                            .ok_or("the clipboard owner sent malformed TARGETS")?;
                        return Ok(targets.any(|target| target == hint));
                    }
                    Some(_) => {}
                    None if Instant::now() >= deadline => {
                        return Err("the clipboard owner did not answer".into())
                    }
                    None => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux::classify_wayland;
    use std::collections::HashSet;
    use wl_clipboard_rs::paste::Error;

    fn offered(types: &[&str]) -> Result<HashSet<String>, Error> {
        Ok(types.iter().map(|t| t.to_string()).collect())
    }

    #[test]
    fn wayland_entry_with_the_kde_hint_is_concealed() {
        let types = offered(&["text/plain;charset=utf-8", "x-kde-passwordManagerHint"]);

        assert_eq!(classify_wayland(types), Ok(Some(true)));
    }

    #[test]
    fn wayland_entry_without_the_hint_is_not_concealed() {
        let types = offered(&["text/plain;charset=utf-8", "UTF8_STRING"]);

        assert_eq!(classify_wayland(types), Ok(Some(false)));
    }

    #[test]
    fn empty_wayland_clipboard_is_not_concealed() {
        assert_eq!(
            classify_wayland(Err(Error::ClipboardEmpty)),
            Ok(Some(false))
        );
        assert_eq!(classify_wayland(Err(Error::NoSeats)), Ok(Some(false)));
    }

    #[test]
    fn compositor_without_data_control_falls_back_to_x11() {
        let missing = Error::MissingProtocol {
            name: "zwlr_data_control_manager_v1",
            version: 1,
        };

        assert_eq!(classify_wayland(Err(missing)), Ok(None));
    }

    #[test]
    fn other_wayland_failures_fail_closed() {
        let broken_pipe = Error::PipeCreation(std::io::Error::other("no fds left"));

        assert!(classify_wayland(Err(broken_pipe))
            .unwrap_err()
            .contains("no fds left"));
        assert!(classify_wayland(Err(Error::NoMimeType)).is_err());
    }
}
