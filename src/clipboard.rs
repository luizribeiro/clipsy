use ::clipboard::{ClipboardContext, ClipboardProvider};
use anyhow::{anyhow, Result};
use std::sync::{Mutex, MutexGuard};

pub trait Clipboard: Send + Sync + 'static {
    fn read(&self) -> Result<String>;
    fn write(&self, content: String) -> Result<()>;
}

pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
    fn read(&self) -> Result<String> {
        open()?
            .get_contents()
            .map_err(|e| anyhow!("reading clipboard: {e}"))
    }

    fn write(&self, content: String) -> Result<()> {
        open()?
            .set_contents(content)
            .map_err(|e| anyhow!("writing clipboard: {e}"))
    }
}

fn open() -> Result<ClipboardContext> {
    ClipboardProvider::new().map_err(|e| anyhow!("opening clipboard: {e}"))
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
}
