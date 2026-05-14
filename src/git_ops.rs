use anyhow::{Context, Result};
use git2::{
    BranchType, Cred, DiffOptions, FetchOptions, PushOptions,
    RemoteCallbacks, Repository, Signature, Sort, Status, StatusOptions, StatusShow,
};
use std::env;
use std::path::Path;

/// Represents remote tracking info (ahead/behind counts)
#[derive(Clone, Debug, Default)]
pub struct RemoteStatus {
    pub ahead: usize,
    pub behind: usize,
    pub remote_name: Option<String>,
    #[allow(dead_code)]
    pub upstream_branch: Option<String>,
}

/// Represents a single commit in the log
#[derive(Clone, Debug)]
pub struct CommitInfo {
    pub id: String,        // short SHA
    pub id_full: String,   // full SHA
    pub message: String,   // first line of commit message
    pub author: String,
    pub time: i64,         // unix timestamp
    pub parents: Vec<String>,
    pub branches: Vec<String>,
    #[allow(dead_code)] // populated in tests + reserved for future UI use
    pub is_head: bool,
}

/// Represents a file's status in the working tree
#[derive(Clone, Debug)]
pub struct FileStatus {
    pub path: String,
    pub status: FileState,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FileState {
    New,
    Modified,
    Deleted,
    Renamed,
    Typechange,
    Conflicted,
}

/// Represents a branch
#[derive(Clone, Debug)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
    pub commit_id: String,
}

/// Opens a repository at the given path (or current dir)
pub fn open_repo(path: Option<&str>) -> Result<Repository> {
    let path = path.unwrap_or(".");
    Repository::discover(path).context("Failed to find a Git repository")
}

/// Get the commit log with branch labels
pub fn get_commits(repo: &Repository, max_count: usize) -> Result<Vec<CommitInfo>> {
    // Empty repo — no HEAD yet
    if repo.head().is_err() {
        return Ok(Vec::new());
    }

    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TIME | Sort::TOPOLOGICAL)?;
    revwalk.push_head()?;

    // Also push all branch tips so we see everything
    for branch in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch?;
        if let Some(target) = branch.get().target() {
            let _ = revwalk.push(target);
        }
    }

    // Build a map of commit SHA -> branch names
    let mut branch_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for branch in repo.branches(None)? {
        let (branch, _btype) = branch?;
        if let (Some(name), Some(target)) = (branch.name()?, branch.get().target()) {
            branch_map
                .entry(target.to_string())
                .or_default()
                .push(name.to_string());
        }
    }

    let head_target = repo.head().ok().and_then(|h| h.target());

    let mut commits = Vec::new();
    for oid in revwalk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let short_id = &oid.to_string()[..7];
        let full_id = oid.to_string();

        let message = commit
            .message()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let author = commit.author().name().unwrap_or("Unknown").to_string();
        let time = commit.time().seconds();

        let parents: Vec<String> = commit
            .parent_ids()
            .map(|p| p.to_string()[..7].to_string())
            .collect();

        let branches = branch_map.get(&full_id).cloned().unwrap_or_default();
        let is_head = head_target.map_or(false, |h| h == oid);

        commits.push(CommitInfo {
            id: short_id.to_string(),
            id_full: full_id,
            message,
            author,
            time,
            parents,
            branches,
            is_head,
        });

        if commits.len() >= max_count {
            break;
        }
    }

    Ok(commits)
}

/// Get the status of files in the working tree
pub fn get_status(repo: &Repository) -> Result<(Vec<FileStatus>, Vec<FileStatus>)> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .show(StatusShow::IndexAndWorkdir);

    let statuses = repo.statuses(Some(&mut opts))?;

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();

    let index_checks: &[(Status, FileState)] = &[
        (Status::INDEX_NEW, FileState::New),
        (Status::INDEX_MODIFIED, FileState::Modified),
        (Status::INDEX_DELETED, FileState::Deleted),
        (Status::INDEX_RENAMED, FileState::Renamed),
        (Status::INDEX_TYPECHANGE, FileState::Typechange),
    ];

    let wt_checks: &[(Status, FileState)] = &[
        (Status::WT_NEW, FileState::New),
        (Status::WT_MODIFIED, FileState::Modified),
        (Status::WT_DELETED, FileState::Deleted),
        (Status::WT_RENAMED, FileState::Renamed),
        (Status::WT_TYPECHANGE, FileState::Typechange),
        (Status::CONFLICTED, FileState::Conflicted),
    ];

    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("???").to_string();
        let st = entry.status();

        for (flag, state) in index_checks {
            if st.contains(*flag) {
                staged.push(FileStatus { path: path.clone(), status: state.clone() });
            }
        }
        for (flag, state) in wt_checks {
            if st.contains(*flag) {
                unstaged.push(FileStatus { path: path.clone(), status: state.clone() });
            }
        }
    }

    Ok((staged, unstaged))
}

/// Stage a file (add to index)
pub fn stage_file(repo: &Repository, path: &str) -> Result<()> {
    let mut index = repo.index()?;
    index.add_path(std::path::Path::new(path))?;
    index.write()?;
    Ok(())
}

/// Unstage a file (remove from index, keep in workdir)
pub fn unstage_file(repo: &Repository, path: &str) -> Result<()> {
    match repo.head().and_then(|h| h.peel_to_commit()) {
        Ok(head) => {
            repo.reset_default(Some(head.as_object()), [path])?;
        }
        Err(_) => {
            // No commits yet — remove from index directly
            let mut index = repo.index()?;
            index.remove_path(std::path::Path::new(path))?;
            index.write()?;
        }
    }
    Ok(())
}

/// Create a commit with the current index
pub fn create_commit(repo: &Repository, message: &str) -> Result<String> {
    let mut index = repo.index()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;

    let sig = repo.signature().or_else(|_| Signature::now("User", "user@example.com"))?;

    let oid = match repo.head().and_then(|h| h.peel_to_commit()) {
        Ok(parent) => repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?,
        Err(_) => repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])?,
    };

    Ok(oid.to_string()[..7].to_string())
}

/// Get list of branches
pub fn get_branches(repo: &Repository) -> Result<Vec<BranchInfo>> {
    let mut branches = Vec::new();
    for branch_result in repo.branches(None)? {
        let (branch, btype) = branch_result?;
        let name = branch.name()?.unwrap_or("???").to_string();
        let is_remote = btype == BranchType::Remote;
        let is_head = branch.is_head();

        let upstream = branch
            .upstream()
            .ok()
            .and_then(|u| u.name().ok().flatten().map(|s| s.to_string()));

        let commit_id = branch
            .get()
            .target()
            .map(|o| o.to_string()[..7].to_string())
            .unwrap_or_default();

        branches.push(BranchInfo {
            name,
            is_head,
            is_remote,
            upstream,
            commit_id,
        });
    }

    // Sort: HEAD first, then local, then remote
    branches.sort_by(|a, b| {
        b.is_head
            .cmp(&a.is_head)
            .then(a.is_remote.cmp(&b.is_remote))
            .then(a.name.cmp(&b.name))
    });

    Ok(branches)
}

/// Create a new branch at HEAD
pub fn create_branch(repo: &Repository, name: &str) -> Result<()> {
    let head = repo.head()?.peel_to_commit()?;
    repo.branch(name, &head, false)?;
    Ok(())
}

/// Switch to a branch
pub fn checkout_branch(repo: &Repository, name: &str) -> Result<()> {
    let (object, reference) = repo.revparse_ext(&format!("refs/heads/{}", name))?;
    repo.checkout_tree(&object, None)?;
    if let Some(reference) = reference {
        repo.set_head(reference.name().unwrap())?;
    }
    Ok(())
}

/// Delete a local branch
pub fn delete_branch(repo: &Repository, name: &str) -> Result<()> {
    let mut branch = repo.find_branch(name, BranchType::Local)?;
    branch.delete()?;
    Ok(())
}

/// Get diff for a specific file
pub fn get_file_diff(repo: &Repository, path: &str, staged: bool) -> Result<String> {
    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(path);

    let diff = if staged {
        let head_tree = repo.head().and_then(|h| h.peel_to_tree()).ok();
        repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut diff_opts))?
    } else {
        repo.diff_index_to_workdir(None, Some(&mut diff_opts))?
    };

    let mut result = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let prefix = match line.origin() {
            '+' => "+",
            '-' => "-",
            ' ' => " ",
            _ => "",
        };
        result.push_str(&format!(
            "{}{}",
            prefix,
            std::str::from_utf8(line.content()).unwrap_or("")
        ));
        true
    })?;

    if result.is_empty() {
        result = "(no diff available)".to_string();
    }

    Ok(result)
}

/// Extract the remote name from an upstream ref like "origin/main" → "origin"
fn parse_remote_name(upstream_name: &str) -> String {
    upstream_name.split('/').next().unwrap_or("origin").to_string()
}

// ── Remote Operations ───────────────────────────────────

/// Build remote callbacks with SSH key or credential discovery
fn make_callbacks<'a>() -> RemoteCallbacks<'a> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|url, username_from_url, allowed_types| {
        // Try SSH agent first
        if allowed_types.is_ssh_key() {
            let user = username_from_url.unwrap_or("git");
            // Try SSH agent
            if let Ok(cred) = Cred::ssh_key_from_agent(user) {
                return Ok(cred);
            }
            // Try default SSH key paths
            let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
            for key_name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
                let key_path = Path::new(&home).join(".ssh").join(key_name);
                if key_path.exists() {
                    if let Ok(cred) = Cred::ssh_key(user, None, &key_path, None) {
                        return Ok(cred);
                    }
                }
            }
        }
        // Try credential helper / default credentials
        if allowed_types.is_user_pass_plaintext() {
            if let Ok(cred) = Cred::credential_helper(
                &git2::Config::open_default()?,
                url,
                username_from_url,
            ) {
                return Ok(cred);
            }
        }
        Cred::default()
    });

    // Transfer progress callback
    callbacks.transfer_progress(|_stats| true);

    callbacks
}

/// Get ahead/behind counts relative to upstream
pub fn get_remote_status(repo: &Repository) -> Result<RemoteStatus> {
    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return Ok(RemoteStatus::default()),
    };

    let branch_name = match head.shorthand() {
        Some(name) => name.to_string(),
        None => return Ok(RemoteStatus::default()),
    };

    let local_branch = repo.find_branch(&branch_name, BranchType::Local)?;

    let upstream = match local_branch.upstream() {
        Ok(u) => u,
        Err(_) => {
            return Ok(RemoteStatus {
                remote_name: None,
                upstream_branch: None,
                ahead: 0,
                behind: 0,
            })
        }
    };

    let upstream_name = upstream
        .name()?
        .map(|s| s.to_string())
        .unwrap_or_default();

    let remote_name = Some(parse_remote_name(&upstream_name));

    let local_oid = head.target().context("HEAD has no target")?;
    let upstream_oid = upstream
        .get()
        .target()
        .context("Upstream has no target")?;

    let (ahead, behind) = repo.graph_ahead_behind(local_oid, upstream_oid)?;

    Ok(RemoteStatus {
        ahead,
        behind,
        remote_name,
        upstream_branch: Some(upstream_name),
    })
}

/// Get the URL of a remote (defaults to "origin" if name is None)
pub fn get_remote_url(repo: &Repository, name: Option<&str>) -> Option<String> {
    let remote_name = name.unwrap_or("origin");
    repo.find_remote(remote_name)
        .ok()
        .and_then(|r| r.url().map(|u| u.to_string()))
}

/// Fetch from the remote tracking the current branch
pub fn fetch(repo: &Repository) -> Result<String> {
    let head = repo.head().context("Cannot fetch: no HEAD")?;
    let branch_name = head.shorthand().context("Cannot determine branch name")?;
    let local_branch = repo.find_branch(branch_name, BranchType::Local)?;
    let upstream = local_branch
        .upstream()
        .context("No upstream branch configured. Set one with: git branch --set-upstream-to=origin/<branch>")?;

    let upstream_name = upstream
        .name()?
        .unwrap_or("origin")
        .to_string();
    let remote_name = parse_remote_name(&upstream_name);

    let mut remote = repo.find_remote(&remote_name)?;

    let callbacks = make_callbacks();
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let refspecs: Vec<String> = remote
        .fetch_refspecs()?
        .iter()
        .filter_map(|s| s.map(|s| s.to_string()))
        .collect();
    let refspec_strs: Vec<&str> = refspecs.iter().map(|s| s.as_str()).collect();

    remote.fetch(&refspec_strs, Some(&mut fetch_opts), None)?;

    Ok(remote_name)
}

/// Pull: fetch + fast-forward merge
pub fn pull(repo: &Repository) -> Result<String> {
    // Step 1: Fetch
    let remote_name = fetch(repo)?;

    // Step 2: Get the updated upstream ref
    let head = repo.head()?;
    let branch_name = head.shorthand().context("No branch name")?;
    let local_branch = repo.find_branch(branch_name, BranchType::Local)?;
    let upstream = local_branch.upstream()?;
    let upstream_oid = upstream.get().target().context("Upstream has no target")?;

    let local_oid = head.target().context("HEAD has no target")?;

    if local_oid == upstream_oid {
        return Ok("Already up to date".to_string());
    }

    // Check if fast-forward is possible
    let (ahead, behind) = repo.graph_ahead_behind(local_oid, upstream_oid)?;

    if ahead > 0 && behind > 0 {
        return Err(anyhow::anyhow!(
            "Cannot fast-forward: local is {} ahead and {} behind. Please merge or rebase manually.",
            ahead,
            behind
        ));
    }

    if behind == 0 {
        return Ok("Already up to date".to_string());
    }

    // Fast-forward: move HEAD to upstream
    let upstream_commit = repo.find_commit(upstream_oid)?;
    let upstream_tree = upstream_commit.tree()?;

    repo.checkout_tree(upstream_tree.as_object(), None)?;

    // Update the branch ref
    let refname = format!("refs/heads/{}", branch_name);
    repo.reference(
        &refname,
        upstream_oid,
        true,
        &format!("pull: fast-forward to {}", &upstream_oid.to_string()[..7]),
    )?;
    repo.set_head(&refname)?;

    Ok(format!(
        "Fast-forwarded {} commits from {}",
        behind, remote_name
    ))
}

/// Push the current branch to its upstream remote
pub fn push(repo: &Repository) -> Result<String> {
    let head = repo.head().context("Cannot push: no HEAD")?;
    let branch_name = head
        .shorthand()
        .context("Cannot determine branch name")?
        .to_string();

    let local_branch = repo.find_branch(&branch_name, BranchType::Local)?;
    let upstream = local_branch
        .upstream()
        .context("No upstream branch configured. Set one with: git push -u origin <branch>")?;

    let upstream_name = upstream
        .name()?
        .unwrap_or("origin")
        .to_string();
    let remote_name = parse_remote_name(&upstream_name);

    let mut remote = repo.find_remote(&remote_name)?;

    let callbacks = make_callbacks();
    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    let refspec = format!("refs/heads/{}:refs/heads/{}", branch_name, branch_name);
    remote.push(&[&refspec], Some(&mut push_opts))?;

    Ok(format!(
        "Pushed {} to {}/{}",
        branch_name, remote_name, branch_name
    ))
}

/// Graph lane assignment for commit visualization
#[derive(Clone, Debug)]
pub struct GraphLane {
    pub lane: usize,
    pub active_lanes: Vec<bool>,
    pub merge_from: Option<usize>,
}

/// Compute graph lane assignments for a list of commits (topological order)
pub fn compute_graph_lanes(commits: &[CommitInfo]) -> Vec<GraphLane> {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut result = Vec::new();

    for commit in commits {
        // Each distinct branch keeps a dedicated column: once a lane goes None
        // (its branch tip closed), we never reuse it for a fresh commit. The
        // commit either lands in a lane already expecting it as a parent, or
        // gets a brand new lane appended at the right.
        let my_lane = lanes
            .iter()
            .position(|l| l.as_deref() == Some(commit.id.as_str()))
            .unwrap_or_else(|| {
                lanes.push(None);
                lanes.len() - 1
            });

        while lanes.len() <= my_lane {
            lanes.push(None);
        }

        let mut merge_from = None;

        match commit.parents.len() {
            0 => {
                lanes[my_lane] = None;
            }
            1 => {
                lanes[my_lane] = Some(commit.parents[0].clone());
            }
            _ => {
                lanes[my_lane] = Some(commit.parents[0].clone());
                let parent2 = &commit.parents[1];
                let p2_lane = lanes
                    .iter()
                    .position(|l| l.as_deref() == Some(parent2.as_str()))
                    .unwrap_or_else(|| {
                        lanes.push(Some(parent2.clone()));
                        lanes.len() - 1
                    });
                merge_from = Some(p2_lane);
            }
        }

        let active_lanes = lanes.iter().map(|l| l.is_some()).collect();

        result.push(GraphLane {
            lane: my_lane,
            active_lanes,
            merge_from,
        });
    }

    result
}

/// Get the current branch name
pub fn get_current_branch(repo: &Repository) -> String {
    repo.head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "detached".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Create a temp directory and init a git repo inside it.
    /// Returns (Repository, TempDir path) — caller should clean up.
    fn temp_repo() -> (Repository, std::path::PathBuf) {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "git_lumina_test_{}_{}", std::process::id(), id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let repo = Repository::init(&dir).unwrap();

        // Configure a user so commits work
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();

        (repo, dir)
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = fs::remove_dir_all(dir);
    }

    /// Helper: write a file, stage it, and commit
    fn write_and_commit(repo: &Repository, dir: &std::path::Path, filename: &str, content: &str, msg: &str) -> String {
        fs::write(dir.join(filename), content).unwrap();
        stage_file(repo, filename).unwrap();
        create_commit(repo, msg).unwrap()
    }

    // ── open_repo ───────────────────────────────────────

    #[test]
    fn given_a_valid_git_repo_when_opening_then_it_succeeds() {
        let (_, dir) = temp_repo();
        let result = open_repo(Some(dir.to_str().unwrap()));
        assert!(result.is_ok());
        cleanup(&dir);
    }

    #[test]
    fn given_an_invalid_path_when_opening_then_it_fails() {
        let result = open_repo(Some("/nonexistent/path/surely"));
        assert!(result.is_err());
    }

    // ── get_commits ─────────────────────────────────────

    #[test]
    fn given_an_empty_repo_when_getting_commits_then_returns_empty_list() {
        let (repo, dir) = temp_repo();
        let commits = get_commits(&repo, 100).unwrap();
        assert!(commits.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn given_two_commits_when_getting_commits_then_returns_them_in_reverse_chronological_order() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "hello", "first commit");
        write_and_commit(&repo, &dir, "b.txt", "world", "second commit");

        let commits = get_commits(&repo, 100).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].message, "second commit");
        assert_eq!(commits[1].message, "first commit");
        assert!(commits[0].is_head);
        assert!(!commits[1].is_head);
        cleanup(&dir);
    }

    #[test]
    fn given_five_commits_when_requesting_three_then_returns_only_three() {
        let (repo, dir) = temp_repo();
        for i in 0..5 {
            write_and_commit(&repo, &dir, &format!("{}.txt", i), "x", &format!("commit {}", i));
        }
        let commits = get_commits(&repo, 3).unwrap();
        assert_eq!(commits.len(), 3);
        cleanup(&dir);
    }

    #[test]
    fn given_a_commit_when_inspecting_sha_then_short_is_7_chars_and_full_is_40_chars() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "data", "init");
        let commits = get_commits(&repo, 1).unwrap();
        assert_eq!(commits[0].id.len(), 7);
        assert_eq!(commits[0].id_full.len(), 40);
        assert!(commits[0].id_full.starts_with(&commits[0].id));
        cleanup(&dir);
    }

    // ── get_status / stage / unstage ────────────────────

    #[test]
    fn given_a_new_untracked_file_when_getting_status_then_it_appears_as_unstaged_new() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "init.txt", "init", "init");
        fs::write(dir.join("new.txt"), "content").unwrap();

        let (staged, unstaged) = get_status(&repo).unwrap();
        assert!(staged.is_empty());
        assert_eq!(unstaged.len(), 1);
        assert_eq!(unstaged[0].path, "new.txt");
        assert_eq!(unstaged[0].status, FileState::New);
        cleanup(&dir);
    }

    #[test]
    fn given_an_unstaged_file_when_staging_it_then_it_moves_to_staged() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "init.txt", "init", "init");
        fs::write(dir.join("new.txt"), "content").unwrap();

        stage_file(&repo, "new.txt").unwrap();
        let (staged, unstaged) = get_status(&repo).unwrap();
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].path, "new.txt");
        assert!(unstaged.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn given_a_staged_file_when_unstaging_it_then_it_moves_back_to_unstaged() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "init.txt", "init", "init");
        fs::write(dir.join("new.txt"), "content").unwrap();

        stage_file(&repo, "new.txt").unwrap();
        unstage_file(&repo, "new.txt").unwrap();
        let (staged, unstaged) = get_status(&repo).unwrap();
        assert!(staged.is_empty());
        assert_eq!(unstaged.len(), 1);
        cleanup(&dir);
    }

    #[test]
    fn given_a_committed_file_when_modifying_it_then_it_appears_as_unstaged_modified() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "file.txt", "original", "init");
        fs::write(dir.join("file.txt"), "modified").unwrap();

        let (staged, unstaged) = get_status(&repo).unwrap();
        assert!(staged.is_empty());
        assert_eq!(unstaged.len(), 1);
        assert_eq!(unstaged[0].status, FileState::Modified);
        cleanup(&dir);
    }

    // ── create_commit ───────────────────────────────────

    #[test]
    fn given_staged_files_when_committing_then_returns_a_7_char_sha() {
        let (repo, dir) = temp_repo();
        fs::write(dir.join("a.txt"), "data").unwrap();
        stage_file(&repo, "a.txt").unwrap();
        let sha = create_commit(&repo, "test commit").unwrap();
        assert_eq!(sha.len(), 7);
        cleanup(&dir);
    }

    #[test]
    fn given_staged_files_when_committing_then_staged_list_becomes_empty() {
        let (repo, dir) = temp_repo();
        fs::write(dir.join("a.txt"), "data").unwrap();
        stage_file(&repo, "a.txt").unwrap();
        create_commit(&repo, "commit it").unwrap();

        let (staged, unstaged) = get_status(&repo).unwrap();
        assert!(staged.is_empty());
        assert!(unstaged.is_empty());
        cleanup(&dir);
    }

    // ── branches ────────────────────────────────────────

    #[test]
    fn given_a_fresh_repo_when_listing_branches_then_only_main_exists_as_head() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "x", "init");
        let branches = get_branches(&repo).unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "main");
        assert!(branches[0].is_head);
        cleanup(&dir);
    }

    #[test]
    fn given_a_repo_when_creating_a_branch_then_it_appears_in_branch_list() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "x", "init");
        create_branch(&repo, "feature").unwrap();

        let branches = get_branches(&repo).unwrap();
        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"feature"));
        assert!(names.contains(&"main"));
        cleanup(&dir);
    }

    #[test]
    fn given_a_new_branch_when_checking_it_out_then_current_branch_changes() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "x", "init");
        create_branch(&repo, "dev").unwrap();
        checkout_branch(&repo, "dev").unwrap();

        assert_eq!(get_current_branch(&repo), "dev");
        cleanup(&dir);
    }

    #[test]
    fn given_an_existing_branch_when_deleting_it_then_it_disappears_from_list() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "x", "init");
        create_branch(&repo, "to-delete").unwrap();
        delete_branch(&repo, "to-delete").unwrap();

        let branches = get_branches(&repo).unwrap();
        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert!(!names.contains(&"to-delete"));
        cleanup(&dir);
    }

    #[test]
    fn given_no_such_branch_when_deleting_then_it_returns_an_error() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "x", "init");
        assert!(delete_branch(&repo, "nope").is_err());
        cleanup(&dir);
    }

    // ── get_current_branch ──────────────────────────────

    #[test]
    fn given_an_empty_repo_when_getting_current_branch_then_returns_detached() {
        let (repo, dir) = temp_repo();
        assert_eq!(get_current_branch(&repo), "detached");
        cleanup(&dir);
    }

    #[test]
    fn given_a_repo_with_commits_when_getting_current_branch_then_returns_a_branch_name() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "x", "init");
        let branch = get_current_branch(&repo);
        assert!(!branch.is_empty());
        assert_ne!(branch, "detached");
        cleanup(&dir);
    }

    // ── get_file_diff ───────────────────────────────────

    #[test]
    fn given_a_modified_file_when_getting_unstaged_diff_then_shows_added_lines() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "file.txt", "line1\n", "init");
        fs::write(dir.join("file.txt"), "line1\nline2\n").unwrap();

        let diff = get_file_diff(&repo, "file.txt", false).unwrap();
        assert!(diff.contains("+line2"));
        cleanup(&dir);
    }

    #[test]
    fn given_a_staged_new_file_when_getting_staged_diff_then_shows_its_content() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "init.txt", "x", "init");
        fs::write(dir.join("new.txt"), "hello\n").unwrap();
        stage_file(&repo, "new.txt").unwrap();

        let diff = get_file_diff(&repo, "new.txt", true).unwrap();
        assert!(diff.contains("+hello"));
        cleanup(&dir);
    }

    #[test]
    fn given_no_changes_when_getting_diff_then_returns_no_diff_available() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "file.txt", "content", "init");

        let diff = get_file_diff(&repo, "file.txt", false).unwrap();
        assert_eq!(diff, "(no diff available)");
        cleanup(&dir);
    }

    // ── get_remote_url / get_remote_status ──────────────

    #[test]
    fn given_no_remote_configured_when_getting_remote_url_then_returns_none() {
        let (repo, dir) = temp_repo();
        assert!(get_remote_url(&repo, None).is_none());
        cleanup(&dir);
    }

    #[test]
    fn given_an_origin_remote_when_getting_its_url_then_returns_the_url() {
        let (repo, dir) = temp_repo();
        repo.remote("origin", "https://example.com/repo.git").unwrap();
        let url = get_remote_url(&repo, Some("origin"));
        assert_eq!(url.as_deref(), Some("https://example.com/repo.git"));
        cleanup(&dir);
    }

    #[test]
    fn given_no_upstream_when_getting_remote_status_then_returns_zeros_and_no_name() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "x", "init");
        let status = get_remote_status(&repo).unwrap();
        assert_eq!(status.ahead, 0);
        assert_eq!(status.behind, 0);
        assert!(status.remote_name.is_none());
        cleanup(&dir);
    }

    #[test]
    fn given_an_empty_repo_when_getting_remote_status_then_returns_default() {
        let (repo, dir) = temp_repo();
        let status = get_remote_status(&repo).unwrap();
        assert_eq!(status.ahead, 0);
        assert_eq!(status.behind, 0);
        cleanup(&dir);
    }

    // ── parse_remote_name ───────────────────────────────

    #[test]
    fn given_origin_main_when_parsing_remote_name_then_returns_origin() {
        assert_eq!(parse_remote_name("origin/main"), "origin");
    }

    #[test]
    fn given_a_name_without_slash_when_parsing_then_returns_the_whole_string() {
        assert_eq!(parse_remote_name("origin"), "origin");
    }

    #[test]
    fn given_upstream_develop_when_parsing_remote_name_then_returns_upstream() {
        assert_eq!(parse_remote_name("upstream/develop"), "upstream");
    }

    // ── compute_graph_lanes ─────────────────────────────

    #[test]
    fn given_no_commits_when_computing_graph_lanes_then_returns_empty() {
        let result = compute_graph_lanes(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn given_a_linear_history_when_computing_lanes_then_all_commits_share_one_lane() {
        let commits = vec![
            CommitInfo {
                id: "aaa".into(), id_full: String::new(), message: String::new(),
                author: String::new(), time: 0, parents: vec!["bbb".into()],
                branches: vec![], is_head: true,
            },
            CommitInfo {
                id: "bbb".into(), id_full: String::new(), message: String::new(),
                author: String::new(), time: 0, parents: vec!["ccc".into()],
                branches: vec![], is_head: false,
            },
            CommitInfo {
                id: "ccc".into(), id_full: String::new(), message: String::new(),
                author: String::new(), time: 0, parents: vec![],
                branches: vec![], is_head: false,
            },
        ];

        let lanes = compute_graph_lanes(&commits);
        assert_eq!(lanes.len(), 3);
        assert_eq!(lanes[0].lane, lanes[1].lane);
        assert_eq!(lanes[1].lane, lanes[2].lane);
        assert!(lanes[0].merge_from.is_none());
        assert!(lanes[1].merge_from.is_none());
    }

    #[test]
    fn given_a_merge_commit_when_computing_lanes_then_parents_are_on_different_lanes() {
        let commits = vec![
            CommitInfo {
                id: "mmm".into(), id_full: String::new(), message: String::new(),
                author: String::new(), time: 0, parents: vec!["aaa".into(), "bbb".into()],
                branches: vec![], is_head: true,
            },
            CommitInfo {
                id: "aaa".into(), id_full: String::new(), message: String::new(),
                author: String::new(), time: 0, parents: vec![],
                branches: vec![], is_head: false,
            },
            CommitInfo {
                id: "bbb".into(), id_full: String::new(), message: String::new(),
                author: String::new(), time: 0, parents: vec![],
                branches: vec![], is_head: false,
            },
        ];

        let lanes = compute_graph_lanes(&commits);
        assert_eq!(lanes.len(), 3);
        assert!(lanes[0].merge_from.is_some());
        assert_ne!(lanes[1].lane, lanes[2].lane);
    }

    // ── branch sorting ──────────────────────────────────

    #[test]
    fn given_multiple_branches_when_listing_then_head_branch_is_first() {
        let (repo, dir) = temp_repo();
        write_and_commit(&repo, &dir, "a.txt", "x", "init");
        create_branch(&repo, "alpha").unwrap();
        create_branch(&repo, "zebra").unwrap();

        let branches = get_branches(&repo).unwrap();
        assert!(branches[0].is_head, "HEAD branch should be first");
        cleanup(&dir);
    }
}

