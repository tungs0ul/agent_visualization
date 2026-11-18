//! The writing desk's console (PLAN.md M20): what changed in the shown session's
//! folder, as git sees it (working tree and index against HEAD, plus new files). Read
//! in a thread when the console opens or on Refresh, never per frame.

use crossbeam_channel::{Receiver, bounded};
use std::process::Command;

use bevy::prelude::Resource;

use crate::server::Waker;

/// Lines of one file's patch shown at most.
const PATCH_LINES: usize = 400;

#[derive(Clone, PartialEq, Debug)]
pub struct FileDiff {
    pub path: String,
    /// None for binary files.
    pub added: Option<u32>,
    pub removed: Option<u32>,
    /// Not tracked by git yet: no patch.
    pub new: bool,
    pub patch: String,
}

pub enum DiffState {
    Empty,
    Loading(Receiver<Result<Vec<FileDiff>, String>>),
    Ready(Vec<FileDiff>),
    Failed(String),
}

#[derive(Resource)]
pub struct DiffView {
    /// The folder the state belongs to.
    pub cwd: Option<String>,
    pub state: DiffState,
    /// The file whose patch is unfolded.
    pub unfolded: Option<String>,
}

impl Default for DiffView {
    fn default() -> Self {
        Self { cwd: None, state: DiffState::Empty, unfolded: None }
    }
}

impl DiffView {
    /// Reads the folder's changes in the background; wakes the app when done.
    pub fn load(&mut self, cwd: &str, waker: Waker) {
        let (sender, receiver) = bounded(1);
        let folder = cwd.to_owned();
        std::thread::spawn(move || {
            let _ = sender.send(read(&folder));
            if let Some(wake) = waker.get() {
                wake();
            }
        });
        self.cwd = Some(cwd.to_owned());
        self.state = DiffState::Loading(receiver);
    }

    /// Takes the result once the thread has it.
    pub fn poll(&mut self) {
        if let DiffState::Loading(receiver) = &self.state
            && let Ok(result) = receiver.try_recv()
        {
            self.state = match result {
                Ok(files) => DiffState::Ready(files),
                Err(error) => DiffState::Failed(error),
            };
        }
    }
}

fn git(cwd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(cwd).args(args).output().map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn read(cwd: &str) -> Result<Vec<FileDiff>, String> {
    // A repository without a commit yet has no HEAD: show the index instead.
    let base: &[&str] = if git(cwd, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok() { &["HEAD"] } else { &[] };
    let numstat = git(cwd, &[&["diff", "--numstat"], base].concat())?;
    let patch = git(cwd, &[&["diff", "--no-color", "--no-ext-diff"], base].concat())?;
    let untracked = git(cwd, &["ls-files", "--others", "--exclude-standard"])?;
    Ok(parse(&numstat, &patch, &untracked))
}

/// Joins `git diff --numstat`, `git diff` and the untracked file list.
fn parse(numstat: &str, patch: &str, untracked: &str) -> Vec<FileDiff> {
    let patches = split_patch(patch);
    let mut files: Vec<FileDiff> = numstat
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let (added, removed, path) = (fields.next()?, fields.next()?, fields.next()?);
            let patch = patches.iter().find(|(file, _)| file == path).map(|(_, text)| text.clone()).unwrap_or_default();
            Some(FileDiff { path: path.to_owned(), added: added.parse().ok(), removed: removed.parse().ok(), new: false, patch })
        })
        .collect();
    files.extend(untracked.lines().filter(|line| !line.is_empty()).map(|path| FileDiff {
        path: path.to_owned(),
        added: None,
        removed: None,
        new: true,
        patch: String::new(),
    }));
    files
}

/// One patch per file, keyed by its new path, each cut to [`PATCH_LINES`] lines.
fn split_patch(patch: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, Vec<&str>)> = Vec::new();
    for line in patch.lines() {
        if let Some(header) = line.strip_prefix("diff --git ") {
            let path = header.rsplit_once(" b/").map_or(header, |(_, path)| path);
            files.push((path.to_owned(), Vec::new()));
        } else if let Some((_, lines)) = files.last_mut()
            && !line.starts_with("index ")
            && !line.starts_with("--- ")
            && !line.starts_with("+++ ")
        {
            lines.push(line);
        }
    }
    files
        .into_iter()
        .map(|(path, lines)| {
            let cut = lines.len() > PATCH_LINES;
            let mut text = lines[..lines.len().min(PATCH_LINES)].join("\n");
            if cut {
                text.push_str(&format!("\n… {} more lines", lines.len() - PATCH_LINES));
            }
            (path, text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_the_counts_the_patches_and_new_files() {
        let numstat = "3\t1\tsrc/a.rs\n-\t-\tlogo.png\n";
        let patch = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1,3 @@\n-old\n+new\n+more\n+lines\ndiff --git a/logo.png b/logo.png\nBinary files differ\n";
        let files = parse(numstat, patch, "notes.md\n");
        assert_eq!(files.len(), 3);
        assert_eq!((files[0].added, files[0].removed), (Some(3), Some(1)));
        assert_eq!(files[0].patch, "@@ -1 +1,3 @@\n-old\n+new\n+more\n+lines");
        assert_eq!(files[1].added, None);
        assert!(files[2].new && files[2].path == "notes.md");
    }

    #[test]
    fn reads_a_real_repository() {
        let dir = std::env::temp_dir().join(format!("diff-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.to_str().unwrap();
        git(cwd, &["init", "-q"]).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        // No commit yet: the new file shows, and nothing fails for want of HEAD.
        let files = read(cwd).unwrap();
        assert_eq!(files.iter().map(|f| (f.path.as_str(), f.new)).collect::<Vec<_>>(), [("a.txt", true)]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
