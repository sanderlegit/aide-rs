# Phase 3: Core Logic - File System and Version Control

## Goal
Implement the standalone modules responsible for interacting with the file system and the version control system (Git). These are core utilities that the agents will depend on.

## Tasks
1.  **Implement File Filtering (`files.rs`)**:
    *   Create the `src/files.rs` module.
    *   Implement the `get_filtered_files` function, which takes a `FileScope`.
    *   Use the `ignore` crate to walk the directory tree.
    *   Configure the `WalkBuilder` with include and exclude glob patterns from the `FileScope`, ensuring it correctly respects `.gitignore` files.
2.  **Implement VCS Operations (`vcs.rs`)**:
    *   Create the `src/vcs.rs` module.
    *   Implement a function to create a Git commit.
    *   Use the `git2` crate to:
        1.  Open the repository in the current directory.
        2.  Add specified files to the index.
        3.  Create a tree from the index.
        4.  Create a commit pointing to the new tree, with the current `HEAD` as its parent.

## Test Coverage
*   **Unit/Integration Tests for `files.rs`**:
    *   Use the `tempfile` crate to create a temporary directory with a nested structure of files and a `.gitignore` file.
    *   Write tests that call `get_filtered_files` with different `FileScope` configurations (e.g., include-only, exclude-only, mixed) and assert that the returned list of `PathBuf`s is exactly correct.
*   **Integration Tests for `vcs.rs`**:
    *   Use `tempfile` to create a temporary directory and initialize a new Git repository within it.
    *   Create and modify a few files.
    *   Write a test that calls the commit function and then uses `git2` to read the latest commit, asserting that its message and author are correct and that the committed tree contains the expected files.

## Completion Criteria
*   All tests for `files.rs` and `vcs.rs` pass.
*   The `get_filtered_files` function correctly identifies files based on glob patterns.
*   The `vcs.rs` module can successfully create a new commit in a Git repository.
