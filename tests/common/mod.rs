use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;

use git2::{Repository, Signature};
use tempfile::{tempdir, TempDir};
use wiremock::MockServer;

#[allow(dead_code)]
pub struct TestEnv {
    pub temp_dir: TempDir,
    pub mock_server: MockServer,
    env_vars: Vec<(String, String)>,
}

#[allow(dead_code)]
impl TestEnv {
    pub async fn new() -> Self {
        let temp_dir = tempdir().unwrap();
        let mock_server = MockServer::start().await;

        // Store env vars to be applied per-command, avoiding race conditions from global state.
        let env_vars = vec![
            ("GEMINI_BASE_URL".to_string(), mock_server.uri()),
            ("GEMINI_API_KEY".to_string(), "test-key".to_string()),
            ("AIDE_RS_TEST_MODE".to_string(), "1".to_string()),
        ];

        Self {
            temp_dir,
            mock_server,
            env_vars,
        }
    }

    pub fn apply_env(&self, cmd: &mut Command) {
        for (key, val) in &self.env_vars {
            cmd.env(key, val);
        }
    }

    pub fn path(&self) -> PathBuf {
        self.temp_dir.path().to_path_buf()
    }

    pub fn full_path(&self, p: &str) -> PathBuf {
        self.path().join(p)
    }

    pub fn create_file(&self, path: &str, content: &str) {
        let full_path = self.full_path(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full_path, content).unwrap();
    }

    pub fn init_git_repo(&self) {
        let repo = Repository::init(self.path()).unwrap();
        let mut index = repo.index().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = Signature::now("Initial", "initial@example.com").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Initial commit",
            &tree,
            &[],
        )
        .unwrap();
    }
}
