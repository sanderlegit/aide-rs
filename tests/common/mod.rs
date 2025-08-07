use std::fs;
use std::path::PathBuf;

use git2::{Repository, Signature};
use tempfile::{tempdir, TempDir};
use wiremock::MockServer;

pub struct TestEnv {
    pub temp_dir: TempDir,
    pub mock_server: MockServer,
}

impl TestEnv {
    pub async fn new() -> Self {
        let temp_dir = tempdir().unwrap();
        let mock_server = MockServer::start().await;

        // Set the env var for the Gemini client to use the mock server
        std::env::set_var("GEMINI_BASE_URL", &mock_server.uri());
        // Set a dummy API key
        std::env::set_var("GEMINI_API_KEY", "test-key");

        Self {
            temp_dir,
            mock_server,
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
