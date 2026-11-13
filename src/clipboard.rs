use anyhow::{anyhow, Result};
use std::sync::{Mutex, MutexGuard};

pub trait Clipboard: Send + Sync + 'static {
    fn read(&self) -> Result<String>;
    fn write(&self, content: String) -> Result<()>;
}

pub trait Handle: Send + 'static {
    fn get_text(&mut self) -> Result<String, arboard::Error>;
    fn set_text(&mut self, content: String) -> Result<(), arboard::Error>;
}

impl Handle for arboard::Clipboard {
    fn get_text(&mut self) -> Result<String, arboard::Error> {
        arboard::Clipboard::get_text(self)
    }

    fn set_text(&mut self, content: String) -> Result<(), arboard::Error> {
        arboard::Clipboard::set_text(self, content)
    }
}

type Opener<H> = Box<dyn Fn() -> Result<H, arboard::Error> + Send + Sync>;

/// Holds one clipboard handle for the life of the process: on X11 the
/// handle owns the selection, so dropping it would discard what was written.
/// A handle that fails is dropped and reopened on the next request, so the
/// server survives a restarted X server or compositor.
pub struct SystemClipboard<H: Handle = arboard::Clipboard> {
    handle: Mutex<Option<H>>,
    open: Opener<H>,
}

impl Default for SystemClipboard {
    fn default() -> Self {
        Self::with_opener(arboard::Clipboard::new)
    }
}

impl<H: Handle> SystemClipboard<H> {
    pub fn with_opener(
        open: impl Fn() -> Result<H, arboard::Error> + Send + Sync + 'static,
    ) -> Self {
        Self {
            handle: Mutex::new(None),
            open: Box::new(open),
        }
    }

    pub fn open(&self) -> Result<()> {
        self.with("opening clipboard", |_| Ok(()))
    }

    fn with<T>(
        &self,
        action: &str,
        op: impl FnOnce(&mut H) -> Result<T, arboard::Error>,
    ) -> Result<T> {
        let mut handle = self.handle.lock().unwrap_or_else(|e| e.into_inner());
        let current = match handle.as_mut() {
            Some(current) => current,
            None => handle.insert((self.open)().map_err(|e| anyhow!("opening clipboard: {e}"))?),
        };
        op(current).map_err(|error| {
            if !matches!(error, arboard::Error::ContentNotAvailable) {
                *handle = None;
            }
            clipboard_error(action, error)
        })
    }
}

impl<H: Handle> Clipboard for SystemClipboard<H> {
    fn read(&self) -> Result<String> {
        self.with("reading clipboard", Handle::get_text)
    }

    fn write(&self, content: String) -> Result<()> {
        self.with("writing clipboard", |handle| handle.set_text(content))
    }
}

fn clipboard_error(action: &str, error: arboard::Error) -> anyhow::Error {
    match error {
        arboard::Error::ContentNotAvailable => anyhow!("clipboard does not hold text"),
        other => anyhow!("{action}: {other}"),
    }
}

#[derive(Default)]
pub struct MemoryClipboard(Mutex<String>);

impl MemoryClipboard {
    pub fn new(content: impl Into<String>) -> Self {
        Self(Mutex::new(content.into()))
    }

    fn content(&self) -> MutexGuard<'_, String> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Clipboard for MemoryClipboard {
    fn read(&self) -> Result<String> {
        Ok(self.content().clone())
    }

    fn write(&self, content: String) -> Result<()> {
        *self.content() = content;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn memory_clipboard_starts_with_initial_content() {
        assert_eq!(
            MemoryClipboard::new("ssh devbox").read().unwrap(),
            "ssh devbox"
        );
    }

    #[test]
    fn memory_clipboard_keeps_last_write() {
        let clipboard = MemoryClipboard::default();

        clipboard.write("first".into()).unwrap();
        clipboard.write("second\n".into()).unwrap();

        assert_eq!(clipboard.read().unwrap(), "second\n");
    }

    #[derive(Default)]
    struct Script {
        opens: usize,
        failing_opens: usize,
        failing_ops: Vec<arboard::Error>,
        text: String,
    }

    struct FakeHandle(Arc<Mutex<Script>>);

    impl FakeHandle {
        fn op(&mut self) -> Result<(), arboard::Error> {
            let mut script = self.0.lock().unwrap();
            if script.failing_ops.is_empty() {
                Ok(())
            } else {
                Err(script.failing_ops.remove(0))
            }
        }
    }

    impl Handle for FakeHandle {
        fn get_text(&mut self) -> Result<String, arboard::Error> {
            self.op()?;
            Ok(self.0.lock().unwrap().text.clone())
        }

        fn set_text(&mut self, content: String) -> Result<(), arboard::Error> {
            self.op()?;
            self.0.lock().unwrap().text = content;
            Ok(())
        }
    }

    fn scripted(script: Script) -> (SystemClipboard<FakeHandle>, Arc<Mutex<Script>>) {
        let script = Arc::new(Mutex::new(script));
        let shared = script.clone();
        let clipboard = SystemClipboard::with_opener(move || {
            let mut state = shared.lock().unwrap();
            state.opens += 1;
            if state.failing_opens > 0 {
                state.failing_opens -= 1;
                return Err(arboard::Error::ClipboardNotSupported);
            }
            Ok(FakeHandle(shared.clone()))
        });
        (clipboard, script)
    }

    fn opens(script: &Arc<Mutex<Script>>) -> usize {
        script.lock().unwrap().opens
    }

    #[test]
    fn one_handle_serves_many_requests() {
        let (clipboard, script) = scripted(Script::default());

        for i in 0..50 {
            clipboard.write(format!("yank {i}")).unwrap();
        }

        assert_eq!(clipboard.read().unwrap(), "yank 49");
        assert_eq!(opens(&script), 1);
    }

    #[test]
    fn failed_open_is_retried_on_the_next_request() {
        let (clipboard, script) = scripted(Script {
            failing_opens: 1,
            ..Script::default()
        });

        let err = clipboard.write("first".into()).unwrap_err().to_string();
        clipboard.write("second".into()).unwrap();

        assert!(err.starts_with("opening clipboard: "), "{err}");
        assert_eq!(clipboard.read().unwrap(), "second");
        assert_eq!(opens(&script), 2);
    }

    #[test]
    fn failing_handle_is_replaced() {
        let (clipboard, script) = scripted(Script {
            failing_ops: vec![arboard::Error::Unknown {
                description: "X11 connection lost".into(),
            }],
            ..Script::default()
        });

        let err = clipboard.write("lost".into()).unwrap_err().to_string();
        clipboard.write("after restart".into()).unwrap();

        assert_eq!(err, "writing clipboard: Unknown error while interacting with the clipboard: X11 connection lost");
        assert_eq!(clipboard.read().unwrap(), "after restart");
        assert_eq!(opens(&script), 2);
    }

    #[test]
    fn non_text_clipboard_keeps_the_handle() {
        let (clipboard, script) = scripted(Script {
            failing_ops: vec![arboard::Error::ContentNotAvailable],
            ..Script::default()
        });

        let err = clipboard.read().unwrap_err().to_string();
        clipboard.write("text again".into()).unwrap();

        assert_eq!(err, "clipboard does not hold text");
        assert_eq!(opens(&script), 1);
    }
}
