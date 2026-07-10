use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use git2::{ObjectType, Repository};

use crate::error::{GitCoreError, Result};
use crate::types::{CommitInfo, EntryKind, TreeEntry};
use crate::validate::validate_slug;

/// Manages the on-disk layout of bare git repositories, storing each
/// repository at `{root}/{owner}/{repo_name}.git`.
#[derive(Debug, Clone)]
pub struct RepoManager {
    root: PathBuf,
}

impl RepoManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Compute the filesystem path for a repository without touching disk.
    pub fn repo_path(&self, owner: &str, name: &str) -> Result<PathBuf> {
        validate_slug(owner)?;
        validate_slug(name)?;
        Ok(self.root.join(owner).join(format!("{name}.git")))
    }

    /// Initialize a new bare repository at `{root}/{owner}/{name}.git`.
    pub fn init_repo(&self, owner: &str, name: &str) -> Result<PathBuf> {
        let path = self.repo_path(owner, name)?;
        if path.exists() {
            return Err(GitCoreError::RepoAlreadyExists(path));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Repository::init_bare(&path)?;
        Ok(path)
    }

    /// Permanently delete a repository from disk.
    pub fn delete_repo(&self, owner: &str, name: &str) -> Result<()> {
        let path = self.repo_path(owner, name)?;
        if !path.exists() {
            return Err(GitCoreError::RepoNotFound(path));
        }
        std::fs::remove_dir_all(&path)?;
        Ok(())
    }

    fn open(&self, owner: &str, name: &str) -> Result<(Repository, PathBuf)> {
        let path = self.repo_path(owner, name)?;
        if !path.exists() {
            return Err(GitCoreError::RepoNotFound(path));
        }
        let repo = Repository::open_bare(&path)?;
        Ok((repo, path))
    }

    /// List local branch names for a repository.
    pub fn list_branches(&self, owner: &str, name: &str) -> Result<Vec<String>> {
        let (repo, _) = self.open(owner, name)?;
        let mut out = Vec::new();
        for branch in repo.branches(Some(git2::BranchType::Local))? {
            let (branch, _) = branch?;
            if let Some(branch_name) = branch.name()? {
                out.push(branch_name.to_string());
            }
        }
        Ok(out)
    }

    /// List tag names for a repository.
    pub fn list_tags(&self, owner: &str, name: &str) -> Result<Vec<String>> {
        let (repo, _) = self.open(owner, name)?;
        let tag_names = repo.tag_names(None)?;
        Ok(tag_names.iter().flatten().map(|s| s.to_string()).collect())
    }

    /// Determine the default branch: prefer HEAD's target, falling back to
    /// `main` or `master` if present.
    pub fn get_default_branch(&self, owner: &str, name: &str) -> Result<String> {
        let (repo, _) = self.open(owner, name)?;
        if let Ok(head) = repo.head() {
            if let Some(shorthand) = head.shorthand() {
                return Ok(shorthand.to_string());
            }
        }
        for candidate in ["main", "master"] {
            if repo
                .find_branch(candidate, git2::BranchType::Local)
                .is_ok()
            {
                return Ok(candidate.to_string());
            }
        }
        Err(GitCoreError::NoDefaultBranch)
    }

    /// Resolve a ref-like string (branch, tag, or sha) to a commit.
    fn resolve_commit<'repo>(
        &self,
        repo: &'repo Repository,
        rev: &str,
    ) -> Result<git2::Commit<'repo>> {
        let obj = repo
            .revparse_single(rev)
            .map_err(|_| GitCoreError::RefNotFound(rev.to_string()))?;
        let commit = obj
            .peel_to_commit()
            .map_err(|_| GitCoreError::RefNotFound(rev.to_string()))?;
        Ok(commit)
    }

    /// Read the raw bytes of a file at a given ref and path.
    pub fn read_file_at_ref(
        &self,
        owner: &str,
        name: &str,
        rev: &str,
        path: &str,
    ) -> Result<Vec<u8>> {
        let (repo, _) = self.open(owner, name)?;
        let commit = self.resolve_commit(&repo, rev)?;
        let tree = commit.tree()?;
        let entry = tree
            .get_path(Path::new(path))
            .map_err(|_| GitCoreError::PathNotFound(path.to_string()))?;
        let object = entry.to_object(&repo)?;
        let blob = object
            .as_blob()
            .ok_or_else(|| GitCoreError::NotAFile(path.to_string()))?;
        Ok(blob.content().to_vec())
    }

    /// List the entries of a directory tree at a given ref and path.
    /// Pass an empty string or "/" for `path` to list the tree root.
    pub fn list_tree(
        &self,
        owner: &str,
        name: &str,
        rev: &str,
        path: &str,
    ) -> Result<Vec<TreeEntry>> {
        let (repo, _) = self.open(owner, name)?;
        let commit = self.resolve_commit(&repo, rev)?;
        let root_tree = commit.tree()?;

        let target_tree = if path.is_empty() || path == "/" {
            root_tree
        } else {
            let entry = root_tree
                .get_path(Path::new(path))
                .map_err(|_| GitCoreError::PathNotFound(path.to_string()))?;
            let object = entry.to_object(&repo)?;
            object
                .as_tree()
                .cloned()
                .ok_or_else(|| GitCoreError::PathNotFound(path.to_string()))?
        };

        let base = if path.is_empty() || path == "/" {
            String::new()
        } else {
            format!("{}/", path.trim_matches('/'))
        };

        let mut out = Vec::new();
        for entry in target_tree.iter() {
            let entry_name = entry.name().unwrap_or_default().to_string();
            let full_path = format!("{base}{entry_name}");
            let oid = entry.id().to_string();
            match entry.kind() {
                Some(ObjectType::Tree) => {
                    out.push(TreeEntry {
                        name: entry_name,
                        path: full_path,
                        kind: EntryKind::Tree,
                        size: None,
                        oid,
                    });
                }
                Some(ObjectType::Blob) => {
                    let size = repo
                        .find_blob(entry.id())
                        .ok()
                        .map(|blob| blob.size() as u64);
                    out.push(TreeEntry {
                        name: entry_name,
                        path: full_path,
                        kind: EntryKind::Blob,
                        size,
                        oid,
                    });
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// Walk the commit history starting at `rev`, returning up to `limit`
    /// commits (most recent first).
    pub fn commit_log(
        &self,
        owner: &str,
        name: &str,
        rev: &str,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let (repo, _) = self.open(owner, name)?;
        let start = self.resolve_commit(&repo, rev)?;

        let mut revwalk = repo.revwalk()?;
        revwalk.push(start.id())?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

        let mut out = Vec::new();
        for oid in revwalk {
            if out.len() >= limit {
                break;
            }
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            let author = commit.author();
            let timestamp = Utc
                .timestamp_opt(commit.time().seconds(), 0)
                .single()
                .unwrap_or_else(Utc::now);
            out.push(CommitInfo {
                sha: commit.id().to_string(),
                author_name: author.name().unwrap_or_default().to_string(),
                author_email: author.email().unwrap_or_default().to_string(),
                message: commit.message().unwrap_or_default().to_string(),
                timestamp,
            });
        }
        Ok(out)
    }

    /// Produce a unified diff (text) between a commit and its first parent
    /// (or against an empty tree if it is a root commit).
    pub fn get_commit_diff(&self, owner: &str, name: &str, sha: &str) -> Result<String> {
        let (repo, _) = self.open(owner, name)?;
        let commit = self.resolve_commit(&repo, sha)?;
        let new_tree = commit.tree()?;
        let old_tree = if commit.parent_count() > 0 {
            Some(commit.parent(0)?.tree()?)
        } else {
            None
        };

        let mut opts = git2::DiffOptions::new();
        let diff = repo.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), Some(&mut opts))?;

        let mut buf = Vec::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            match line.origin() {
                '+' | '-' | ' ' => buf.push(line.origin() as u8),
                _ => {}
            }
            buf.extend_from_slice(line.content());
            true
        })?;

        Ok(String::from_utf8(buf)?)
    }

    /// Produce an in-memory tar archive of the full tree at `git_ref`,
    /// recursively including every blob with its original file mode.
    pub fn archive_tree_at_ref(&self, owner: &str, name: &str, git_ref: &str) -> Result<Vec<u8>> {
        let (repo, _) = self.open(owner, name)?;
        let commit = self.resolve_commit(&repo, git_ref)?;
        let tree = commit.tree()?;

        let mut builder = tar::Builder::new(Vec::new());
        Self::write_tree_to_tar(&repo, &tree, "", &mut builder)?;

        let data = builder.into_inner()?;
        Ok(data)
    }

    /// Merge `source_branch` into `target_branch`. If the target is already
    /// up-to-date with the source, this is a no-op and returns the target's
    /// current commit sha. If a fast-forward is possible, the target branch
    /// ref is simply moved to the source's commit. Otherwise a real merge
    /// commit is created with both branch tips as parents. Returns the sha
    /// of the resulting commit (as hex string).
    #[allow(clippy::too_many_arguments)]
    pub fn merge_branches(
        &self,
        owner: &str,
        name: &str,
        source_branch: &str,
        target_branch: &str,
        author_name: &str,
        author_email: &str,
        message: &str,
    ) -> Result<String> {
        let (repo, _) = self.open(owner, name)?;

        let source_commit = self.resolve_commit(&repo, source_branch)?;
        let target_commit = self.resolve_commit(&repo, target_branch)?;

        let target_ref_name = format!("refs/heads/{target_branch}");

        // `merge_analysis` operates against the repo's current HEAD, which
        // isn't necessarily `target_branch`, so determine fast-forward /
        // up-to-date status directly via ancestry checks instead.
        if source_commit.id() == target_commit.id()
            || repo.graph_descendant_of(target_commit.id(), source_commit.id())?
        {
            // Target already contains source's history: no-op.
            return Ok(target_commit.id().to_string());
        }

        if repo.graph_descendant_of(source_commit.id(), target_commit.id())? {
            // Source is strictly ahead of target: fast-forward.
            let mut target_ref = repo.find_reference(&target_ref_name)?;
            target_ref.set_target(source_commit.id(), "fast-forward merge")?;
            return Ok(source_commit.id().to_string());
        }

        // Real merge commit.
        let base_oid = repo.merge_base(target_commit.id(), source_commit.id())?;
        let base_commit = repo.find_commit(base_oid)?;

        let ancestor_tree = base_commit.tree()?;
        let target_tree = target_commit.tree()?;
        let source_tree = source_commit.tree()?;

        let mut index = repo.merge_trees(&ancestor_tree, &target_tree, &source_tree, None)?;

        if index.has_conflicts() {
            return Err(GitCoreError::MergeConflict(
                source_branch.to_string(),
                target_branch.to_string(),
            ));
        }

        let tree_oid = index.write_tree_to(&repo)?;
        let tree = repo.find_tree(tree_oid)?;

        let signature = git2::Signature::now(author_name, author_email)?;

        let commit_oid = repo.commit(
            Some(&target_ref_name),
            &signature,
            &signature,
            message,
            &tree,
            &[&target_commit, &source_commit],
        )?;

        Ok(commit_oid.to_string())
    }

    /// Recursively walk `tree`, appending every blob it (transitively)
    /// contains to `builder` as a tar entry under `prefix`.
    fn write_tree_to_tar(
        repo: &Repository,
        tree: &git2::Tree,
        prefix: &str,
        builder: &mut tar::Builder<Vec<u8>>,
    ) -> Result<()> {
        for entry in tree.iter() {
            let entry_name = entry.name().unwrap_or_default().to_string();
            let full_path = format!("{prefix}{entry_name}");
            match entry.kind() {
                Some(ObjectType::Tree) => {
                    let object = entry.to_object(repo)?;
                    let subtree = object
                        .as_tree()
                        .ok_or_else(|| GitCoreError::PathNotFound(full_path.clone()))?;
                    Self::write_tree_to_tar(repo, subtree, &format!("{full_path}/"), builder)?;
                }
                Some(ObjectType::Blob) => {
                    let object = entry.to_object(repo)?;
                    let blob = object
                        .as_blob()
                        .ok_or_else(|| GitCoreError::NotAFile(full_path.clone()))?;
                    let mode = entry.filemode() as u32;
                    let mut header = tar::Header::new_gnu();
                    header.set_size(blob.content().len() as u64);
                    header.set_mode(mode);
                    header.set_cksum();
                    builder.append_data(&mut header, &full_path, blob.content())?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    /// Create a fresh `RepoManager` rooted at a temp dir, plus a bare repo
    /// under it with a single commit containing one file.
    fn setup_repo_with_commit() -> (tempfile::TempDir, RepoManager, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = RepoManager::new(dir.path());
        manager.init_repo("acme", "widgets").expect("init repo");
        let repo_path = manager.repo_path("acme", "widgets").expect("repo path");
        let repo = Repository::open_bare(&repo_path).expect("open bare repo");

        let mut index = repo.index().expect("index");
        let blob_oid = repo
            .blob(b"hello from archive test\n")
            .expect("write blob");
        index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                file_size: 24,
                id: blob_oid,
                flags: 0,
                flags_extended: 0,
                path: b"hello.txt".to_vec(),
            })
            .expect("add index entry");
        let tree_oid = index.write_tree_to(&repo).expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let sig = git2::Signature::now("Test", "test@example.com").expect("signature");
        let commit_oid = repo
            .commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
            .expect("commit");

        (dir, manager, commit_oid.to_string())
    }

    #[test]
    fn archive_tree_at_ref_produces_expected_tar() {
        let (_dir, manager, commit_sha) = setup_repo_with_commit();

        let archive_bytes = manager
            .archive_tree_at_ref("acme", "widgets", &commit_sha)
            .expect("archive tree");

        let mut archive = tar::Archive::new(archive_bytes.as_slice());
        let entries = archive.entries().expect("entries");

        let mut found = false;
        for entry in entries {
            let mut entry = entry.expect("entry");
            let path = entry.path().expect("path").to_string_lossy().to_string();
            if path == "hello.txt" {
                let mut contents = String::new();
                entry.read_to_string(&mut contents).expect("read entry");
                assert_eq!(contents, "hello from archive test\n");
                found = true;
            }
        }
        assert!(found, "expected hello.txt entry in tar archive");
    }

    /// Commit a single file with given content on top of `parent_oid` (or
    /// as a root commit if `parent_oid` is `None`), updating the given ref.
    fn commit_file(
        repo: &Repository,
        ref_name: &str,
        parent_oid: Option<git2::Oid>,
        file_name: &str,
        content: &[u8],
    ) -> git2::Oid {
        let mut index = git2::Index::new().expect("new index");
        if let Some(parent) = parent_oid {
            let parent_commit = repo.find_commit(parent).expect("find parent");
            index
                .read_tree(&parent_commit.tree().expect("parent tree"))
                .expect("read tree into index");
        }
        let blob_oid = repo.blob(content).expect("write blob");
        index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                file_size: content.len() as u32,
                id: blob_oid,
                flags: 0,
                flags_extended: 0,
                path: file_name.as_bytes().to_vec(),
            })
            .expect("add index entry");
        let tree_oid = index.write_tree_to(repo).expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let sig = git2::Signature::now("Test", "test@example.com").expect("signature");
        let parents: Vec<git2::Commit> = parent_oid
            .map(|oid| vec![repo.find_commit(oid).expect("find parent")])
            .unwrap_or_default();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some(ref_name), &sig, &sig, "commit", &tree, &parent_refs)
            .expect("commit")
    }

    #[test]
    fn merge_branches_fast_forwards_when_possible() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = RepoManager::new(dir.path());
        manager.init_repo("acme", "widgets").expect("init repo");
        let repo_path = manager.repo_path("acme", "widgets").expect("repo path");
        let repo = Repository::open_bare(&repo_path).expect("open bare repo");

        let base_oid = commit_file(&repo, "refs/heads/main", None, "a.txt", b"base\n");
        repo.reference("refs/heads/feature", base_oid, false, "create feature")
            .expect("create feature branch");

        let feature_oid = commit_file(
            &repo,
            "refs/heads/feature",
            Some(base_oid),
            "b.txt",
            b"feature change\n",
        );

        let result_sha = manager
            .merge_branches(
                "acme",
                "widgets",
                "feature",
                "main",
                "Merger",
                "merger@example.com",
                "Merge feature into main",
            )
            .expect("merge branches");

        assert_eq!(result_sha, feature_oid.to_string());

        let main_ref = repo
            .find_reference("refs/heads/main")
            .expect("find main ref");
        assert_eq!(main_ref.target().expect("target"), feature_oid);
    }

    #[test]
    fn merge_branches_creates_merge_commit_when_diverged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = RepoManager::new(dir.path());
        manager.init_repo("acme", "widgets").expect("init repo");
        let repo_path = manager.repo_path("acme", "widgets").expect("repo path");
        let repo = Repository::open_bare(&repo_path).expect("open bare repo");

        let base_oid = commit_file(&repo, "refs/heads/main", None, "a.txt", b"base\n");
        repo.reference("refs/heads/feature", base_oid, false, "create feature")
            .expect("create feature branch");

        // Diverge: commit on main and a different commit on feature.
        let main_oid = commit_file(
            &repo,
            "refs/heads/main",
            Some(base_oid),
            "main-only.txt",
            b"main change\n",
        );
        let feature_oid = commit_file(
            &repo,
            "refs/heads/feature",
            Some(base_oid),
            "feature-only.txt",
            b"feature change\n",
        );

        let result_sha = manager
            .merge_branches(
                "acme",
                "widgets",
                "feature",
                "main",
                "Merger",
                "merger@example.com",
                "Merge feature into main",
            )
            .expect("merge branches");

        let merge_commit = repo
            .find_commit(git2::Oid::from_str(&result_sha).expect("parse oid"))
            .expect("find merge commit");
        assert_eq!(merge_commit.parent_count(), 2);
        assert_eq!(merge_commit.parent_id(0).expect("parent0"), main_oid);
        assert_eq!(merge_commit.parent_id(1).expect("parent1"), feature_oid);

        let main_ref = repo
            .find_reference("refs/heads/main")
            .expect("find main ref");
        assert_eq!(main_ref.target().expect("target"), merge_commit.id());
    }
}
