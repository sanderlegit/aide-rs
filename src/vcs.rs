use crate::error::Result;
use git2::{Oid, Repository, Signature};
use std::path::{Path, PathBuf};

pub fn add_and_commit(repo_path: &Path, paths: &[PathBuf], message: &str) -> Result<Oid> {
    let repo = Repository::open(repo_path)?;

    let mut index = repo.index()?;

    for path in paths {
        // We need to make the path relative to the repo's workdir.
        let relative_path = path.strip_prefix(repo.workdir().unwrap())?;
        index.add_path(relative_path)?;
    }

    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let head = repo.head()?.peel_to_commit()?;
    let signature = Signature::now("AI Agent", "ai-agent@example.com")?;

    Ok(repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &[&head],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_add_and_commit() -> crate::error::Result<()> {
        let dir = tempdir().unwrap();
        let repo_path = dir.path();

        // 1. Initialize a new git repository
        let repo = Repository::init(repo_path)?;

        // 2. Create an initial commit so HEAD exists
        let mut index = repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let signature = Signature::now("Initial", "initial@example.com")?;
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Initial commit",
            &tree,
            &[],
        )?;

        // 3. Create and modify some files
        let file1_path = repo_path.join("file1.txt");
        let mut file1 = File::create(&file1_path)?;
        writeln!(file1, "hello")?;

        let subdir = repo_path.join("subdir");
        fs::create_dir(&subdir)?;
        let file2_path = subdir.join("file2.txt");
        let mut file2 = File::create(&file2_path)?;
        writeln!(file2, "world")?;

        // 4. Call our function to add and commit them
        let paths_to_commit = vec![file1_path.clone(), file2_path.clone()];
        let commit_message = "Add test files";
        let commit_oid = add_and_commit(repo_path, &paths_to_commit, commit_message)?;

        // 5. Verify the commit
        let commit = repo.find_commit(commit_oid)?;
        assert_eq!(commit.message(), Some(commit_message));

        let tree = commit.tree()?;
        assert!(tree.get_path(Path::new("file1.txt")).is_ok());
        assert!(tree.get_path(Path::new("subdir/file2.txt")).is_ok());
        assert!(tree.get_path(Path::new("nonexistent.txt")).is_err());

        Ok(())
    }
}
