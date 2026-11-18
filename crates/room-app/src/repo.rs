//! Which repository (and branch) a tool call works in (PLAN.md M4d): from the paths in
//! its input, walk up to the nearest `.git` and read the branch from `HEAD`. Results
//! are cached per directory, so this costs a few `stat`s per new directory, no git
//! process.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use room_proto::event::ToolCall;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub name: String,
    pub branch: Option<String>,
    pub root: PathBuf,
    /// A linked git worktree (`git worktree add`), not the main checkout: its name.
    pub worktree: Option<String>,
}

#[derive(Resource, Default)]
pub struct RepoCache {
    by_dir: HashMap<PathBuf, Option<Repo>>,
}

impl RepoCache {
    /// The repository that contains `path` (a file or directory), if any.
    pub fn locate(&mut self, path: &Path) -> Option<Repo> {
        let dir = if path.is_dir() { path.to_path_buf() } else { path.parent()?.to_path_buf() };
        if let Some(known) = self.by_dir.get(&dir) {
            return known.clone();
        }
        let found = dir.ancestors().find_map(|candidate| {
            let git = candidate.join(".git");
            git.exists().then(|| Repo {
                name: candidate.file_name().map_or_else(|| "/".to_owned(), |n| n.to_string_lossy().into_owned()),
                branch: read_branch(&git),
                root: candidate.to_path_buf(),
                worktree: worktree_name(&git),
            })
        });
        self.by_dir.insert(dir, found.clone());
        found
    }

    /// The repository a tool call works in: the first of its paths inside one.
    pub fn for_tool(&mut self, tool: &ToolCall, cwd: Option<&str>) -> Option<Repo> {
        paths_in(tool, cwd).iter().find_map(|path| self.locate(path))
    }
}

/// A linked worktree's `.git` is a file, `gitdir: <main>/.git/worktrees/<name>`
/// (a submodule's points into `.git/modules/` instead).
fn worktree_name(git: &Path) -> Option<String> {
    let pointer = fs::read_to_string(git).ok()?;
    let target = pointer.trim().strip_prefix("gitdir:")?.trim();
    let (_, name) = target.trim_end_matches('/').rsplit_once("/worktrees/")?;
    (!name.contains('/')).then(|| name.to_owned())
}

/// `ref: refs/heads/main` ⇒ "main"; a detached HEAD ⇒ its short hash. `.git` may also
/// be a file pointing elsewhere (worktrees, submodules).
fn read_branch(git: &Path) -> Option<String> {
    let git_dir = if git.is_file() {
        let pointer = fs::read_to_string(git).ok()?;
        let target = PathBuf::from(pointer.trim().strip_prefix("gitdir:")?.trim());
        if target.is_absolute() { target } else { git.parent()?.join(target) }
    } else {
        git.to_path_buf()
    };
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => branch.to_owned(),
        None => head.chars().take(7).collect(),
    })
}

/// Paths a tool call touches: file fields, and for Bash the `cd` target and absolute
/// or home-relative paths in the command.
pub fn paths_in(tool: &ToolCall, cwd: Option<&str>) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let resolve = |raw: &str| -> Option<PathBuf> {
        let raw = raw.trim_matches(|c| matches!(c, '"' | '\'' | ';' | '&' | '|' | '(' | ')'));
        if let Some(rest) = raw.strip_prefix("~/") {
            return home.as_ref().map(|home| home.join(rest));
        }
        if raw.starts_with('/') {
            return Some(PathBuf::from(raw));
        }
        None
    };
    let mut paths: Vec<PathBuf> = ["file_path", "notebook_path", "path"]
        .iter()
        .filter_map(|key| tool.input.get(key).and_then(|v| v.as_str()))
        .filter_map(|raw| resolve(raw).or_else(|| cwd.map(|cwd| Path::new(cwd).join(raw))))
        .collect();
    if let Some(command) = tool.input.get("command").and_then(|v| v.as_str()) {
        let words: Vec<&str> = command.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            if *word == "cd"
                && let Some(target) = words.get(i + 1)
            {
                paths.extend(resolve(target).or_else(|| cwd.map(|cwd| Path::new(cwd).join(target))));
            } else if let Some(path) = resolve(word) {
                paths.push(path);
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_repo(name: &str, head: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("agent-room-repo-{}-{name}", std::process::id())).join(name);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".git/HEAD"), head).unwrap();
        root
    }

    #[test]
    fn finds_repo_and_branch_from_a_file_path() {
        let root = temp_repo("shop", "ref: refs/heads/feature/cart\n");
        let mut cache = RepoCache::default();
        let tool = ToolCall { name: "Edit".into(), input: json!({ "file_path": root.join("src/main.rs") }) };
        let repo = cache.for_tool(&tool, None).unwrap();
        assert_eq!((repo.name.as_str(), repo.branch.as_deref()), ("shop", Some("feature/cart")));
        fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }

    #[test]
    fn bash_cd_and_absolute_paths_count() {
        let tool = ToolCall { name: "Bash".into(), input: json!({ "command": "cd ../api && cat /etc/hosts" }) };
        let paths = paths_in(&tool, Some("/work/web"));
        assert_eq!(paths, [PathBuf::from("/work/web/../api"), PathBuf::from("/etc/hosts")]);
    }

    #[test]
    fn detached_head_shows_the_short_hash() {
        let root = temp_repo("lib", "0123456789abcdef\n");
        assert_eq!(read_branch(&root.join(".git")).as_deref(), Some("0123456"));
        fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_linked_worktree_is_named_and_a_submodule_is_not() {
        let base = std::env::temp_dir().join(format!("agent-room-worktree-{}", std::process::id()));
        let tree = base.join("feature");
        fs::create_dir_all(&tree).unwrap();
        fs::write(tree.join(".git"), "gitdir: /work/shop/.git/worktrees/feature\n").unwrap();
        let repo = RepoCache::default().locate(&tree).unwrap();
        assert_eq!(repo.worktree.as_deref(), Some("feature"));
        fs::write(tree.join(".git"), "gitdir: ../.git/modules/lib\n").unwrap();
        assert_eq!(worktree_name(&tree.join(".git")), None);
        fs::remove_dir_all(&base).unwrap();
    }
}
