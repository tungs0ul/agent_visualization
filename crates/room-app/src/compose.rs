//! The chat's composer (PLAN.md M24): `@` file mentions suggested from the session's
//! folder, and files dropped on the window (images attached to the message, other
//! files mentioned by path). The folder's file list is read once per folder, in a
//! thread, the first time the user types `@`.

use std::path::Path;
use std::process::Command;

use base64::Engine as _;
use crossbeam_channel::{Receiver, bounded};

use crate::server::Waker;

/// Suggestions shown under the input at most.
pub const SUGGESTIONS: usize = 8;
/// Files listed at most (a huge monorepo stays cheap to search).
const MAX_FILES: usize = 50_000;
/// Largest image attached: base64 makes it a third bigger, and the API takes 5 MB.
const MAX_IMAGE_BYTES: u64 = 3_750_000;

#[derive(Default)]
pub struct FileIndex {
    cwd: String,
    loading: Option<Receiver<Vec<String>>>,
    files: Vec<String>,
}

impl FileIndex {
    /// Starts reading `cwd`'s files unless they are read already or being read.
    pub fn ensure(&mut self, cwd: &str, waker: Waker) {
        if self.cwd == cwd {
            return;
        }
        let (sender, receiver) = bounded(1);
        let folder = cwd.to_owned();
        std::thread::spawn(move || {
            let _ = sender.send(list_files(&folder));
            if let Some(wake) = waker.get() {
                wake();
            }
        });
        self.cwd = cwd.to_owned();
        self.files.clear();
        self.loading = Some(receiver);
    }

    pub fn poll(&mut self) {
        if let Some(receiver) = &self.loading
            && let Ok(files) = receiver.try_recv()
        {
            self.files = files;
            self.loading = None;
        }
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// The best matches for what follows the `@`: file names starting with it first,
    /// then names containing it, then paths containing it; shorter paths first.
    pub fn suggest(&self, query: &str) -> Vec<&str> {
        rank(&self.files, query)
    }
}

fn rank<'a>(files: &'a [String], query: &str) -> Vec<&'a str> {
    let query = query.to_lowercase();
    let mut scored: Vec<(u8, usize, &str)> = files
        .iter()
        .filter_map(|path| {
            let lower = path.to_lowercase();
            let name = lower.rsplit('/').next().unwrap_or(&lower);
            let score = if name.starts_with(&query) {
                0
            } else if name.contains(&query) {
                1
            } else if lower.contains(&query) {
                2
            } else {
                return None;
            };
            Some((score, path.len(), path.as_str()))
        })
        .collect();
    scored.sort();
    scored.into_iter().take(SUGGESTIONS).map(|(_, _, path)| path).collect()
}

/// The folder's files, relative: git's view (tracked and new, ignored ones left out),
/// or nothing outside a repository.
fn list_files(cwd: &str) -> Vec<String> {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let mut files: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().take(MAX_FILES).map(str::to_owned).collect();
    files.dedup();
    files
}

/// The `@word` being typed at the end of the input: its byte offset and the text
/// after the `@`. None once a space ends it.
pub fn mention_query(input: &str) -> Option<(usize, &str)> {
    let start = input.rfind(char::is_whitespace).map_or(0, |at| at + input[at..].chars().next().map_or(1, char::len_utf8));
    let word = &input[start..];
    word.strip_prefix('@').map(|query| (start, query))
}

/// Replaces the `@word` at `start` with the chosen path and a space.
pub fn complete(input: &mut String, start: usize, path: &str) {
    input.truncate(start);
    input.push('@');
    input.push_str(path);
    input.push(' ');
}

/// An image going with the next message.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub media_type: &'static str,
    /// Base64 of the file.
    pub data: String,
}

impl Attachment {
    /// The message content block for it.
    pub fn block(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "image",
            "source": { "type": "base64", "media_type": self.media_type, "data": self.data },
        })
    }
}

pub fn image_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Reads an image to attach; refuses ones the API would refuse.
pub fn attach(path: &Path) -> Result<Attachment, String> {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    let media_type = image_type(path).ok_or_else(|| format!("{name} is not a PNG, JPEG, GIF or WebP image"))?;
    let size = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?.len();
    if size > MAX_IMAGE_BYTES {
        return Err(format!("{name} is too big ({} MB; at most 3.7 MB)", size / 1_000_000));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    Ok(Attachment { name, media_type, data: base64::engine::general_purpose::STANDARD.encode(bytes) })
}

/// A dropped file that isn't an image goes into the message as `@path`.
pub fn mention_path(path: &Path) -> String {
    let text = path.display().to_string();
    if text.contains(char::is_whitespace) { format!("@\"{text}\"") } else { format!("@{text}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_being_typed_is_the_query() {
        assert_eq!(mention_query("look at @src/ma"), Some((8, "src/ma")));
        assert_eq!(mention_query("@"), Some((0, "")));
        assert_eq!(mention_query("look at @main.rs "), None);
        assert_eq!(mention_query("mail me@x"), None);
        assert_eq!(mention_query("ünï @x"), Some((6, "x")));
        let mut input = "look at @ma".to_owned();
        complete(&mut input, 8, "src/main.rs");
        assert_eq!(input, "look at @src/main.rs ");
    }

    #[test]
    fn names_that_start_with_the_query_come_first() {
        let files: Vec<String> =
            ["docs/main-notes.md", "src/domain.rs", "crates/app/src/main.rs", "src/main.rs", "README.md"].map(str::to_owned).into();
        assert_eq!(rank(&files, "MAIN"), vec!["src/main.rs", "docs/main-notes.md", "crates/app/src/main.rs", "src/domain.rs"]);
        assert_eq!(rank(&files, "docs/"), vec!["docs/main-notes.md"]);
    }

    #[test]
    fn images_attach_and_other_files_are_mentioned() {
        let dir = std::env::temp_dir().join(format!("agent-room-compose-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("shot.PNG");
        std::fs::write(&png, [0x89, b'P', b'N', b'G']).unwrap();
        let image = attach(&png).unwrap();
        assert_eq!((image.name.as_str(), image.media_type, image.data.as_str()), ("shot.PNG", "image/png", "iVBORw=="));
        assert_eq!(image.block()["source"]["media_type"], "image/png");
        assert!(attach(&dir.join("notes.txt")).is_err());
        assert_eq!(mention_path(Path::new("/a b/c.txt")), "@\"/a b/c.txt\"");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
