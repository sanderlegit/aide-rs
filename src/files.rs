use crate::{agents::state::FileScope, error::Result};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

pub fn get_filtered_files(base_dir: &Path, scope: &FileScope) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut builder = WalkBuilder::new(base_dir);

    // We don't want to ignore hidden files by default, but we do respect .gitignore
    builder.hidden(false);

    let mut override_builder = ignore::overrides::OverrideBuilder::new(base_dir);
    for pattern in &scope.include {
        override_builder.add(pattern)?;
    }
    for pattern in &scope.exclude {
        let negated_pattern = format!("!{}", pattern);
        override_builder.add(&negated_pattern)?;
    }
    let overrides = override_builder.build()?;
    builder.overrides(overrides);

    for result in builder.build() {
        let entry = result?;
        if entry.file_type().map_or(false, |ft| ft.is_file()) {
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::state::FileScope;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_get_filtered_files() -> crate::error::Result<()> {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create some files and directories
        fs::create_dir_all(base.join("src/components")).unwrap();
        File::create(base.join("src/main.rs")).unwrap();
        File::create(base.join("src/lib.rs")).unwrap();
        File::create(base.join("src/components/button.rs")).unwrap();
        File::create(base.join("README.md")).unwrap();
        File::create(base.join("config.toml")).unwrap();
        fs::create_dir(base.join(".hidden_dir")).unwrap();
        File::create(base.join(".hidden_dir/secret.txt")).unwrap();

        // Create a .gitignore
        let mut gitignore = File::create(base.join(".gitignore")).unwrap();
        writeln!(gitignore, "config.toml").unwrap();

        // Test case 1: Include all .rs files, exclude main.rs
        let scope1 = FileScope {
            include: vec!["**/*.rs".to_string()],
            exclude: vec!["src/main.rs".to_string()],
        };
        let mut files1 = get_filtered_files(base, &scope1)?;
        let mut expected1 = vec![
            base.join("src/components/button.rs"),
            base.join("src/lib.rs"),
        ];
        files1.sort();
        expected1.sort();
        assert_eq!(files1, expected1);

        // Test case 2: Include markdown and toml files
        let scope2 = FileScope {
            include: vec!["**/*.md".to_string(), "**/*.toml".to_string()],
            exclude: vec![],
        };
        let files2 = get_filtered_files(base, &scope2)?;
        // config.toml should be ignored due to .gitignore
        let expected2 = vec![base.join("README.md")];
        assert_eq!(files2, expected2);

        // Test case 3: Include everything in src, but exclude components
        let scope3 = FileScope {
            include: vec!["src/**/*".to_string()],
            exclude: vec!["src/components/**/*".to_string()],
        };
        let mut files3 = get_filtered_files(base, &scope3)?;
        let mut expected3 = vec![base.join("src/lib.rs"), base.join("src/main.rs")];
        files3.sort();
        expected3.sort();
        assert_eq!(files3, expected3);

        // Test case 4: Include hidden files if explicitly asked
        let scope4 = FileScope {
            include: vec!["**/*.txt".to_string()],
            exclude: vec![],
        };
        let files4 = get_filtered_files(base, &scope4)?;
        let expected4 = vec![base.join(".hidden_dir/secret.txt")];
        assert_eq!(files4, expected4);

        Ok(())
    }
}
