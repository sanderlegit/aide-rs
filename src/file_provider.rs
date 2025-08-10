use crate::error::Result;
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use std::fs;
use std::path::Path;

/// Expands a list of input paths into a list of files, applying filtering rules.
///
/// It walks directories and applies include/exclude rules from a filter file.
/// The default filter file is `.ai/filter=all`.
///
/// # Arguments
///
/// * `paths`: A slice of strings representing file or directory paths.
/// * `project_root_override`: An optional path to use as the project root, for testing.
pub fn get_files(paths: &[String], project_root_override: Option<&Path>) -> Result<Vec<String>> {
    let project_root = match project_root_override {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let mut override_builder = OverrideBuilder::new(&project_root);

    let filter_file_path = project_root.join(".ai/filter=all");
    if filter_file_path.exists() {
        let content = fs::read_to_string(filter_file_path)?;
        let mut is_exclude = false;
        let mut section_found = false;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty()
                || (!line.starts_with("#exclude") && !line.starts_with("#include") && line.starts_with('#'))
            {
                continue;
            }

            if line == "#exclude" {
                is_exclude = true;
                section_found = true;
                continue;
            }
            if line == "#include" {
                is_exclude = false;
                section_found = true;
                continue;
            }

            if section_found {
                let pattern = if is_exclude {
                    format!("!{}", line)
                } else {
                    line.to_string()
                };
                override_builder.add(&pattern)?;
            }
        }
    } else {
        // Default behavior if no filter file is found: include everything, respect .gitignore
        override_builder.add("!/.git")?;
        override_builder.add("!/.ai")?;
    }

    let overrides = override_builder.build()?;
    let mut collected_files = Vec::new();

    for path_str in paths {
        let path = Path::new(path_str);
        let mut walk_builder = WalkBuilder::new(path);
        walk_builder.overrides(overrides.clone());

        for result in walk_builder.build() {
            let entry = result?;
            if entry.file_type().map_or(false, |ft| ft.is_file()) {
                collected_files.push(entry.path().to_string_lossy().to_string());
            }
        }
    }

    collected_files.sort();
    collected_files.dedup();

    Ok(collected_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_get_files_with_filter() -> crate::error::Result<()> {
        let dir = tempdir()?;
        let root = dir.path();

        // Create some files and directories
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/main.rs"), "fn main() {}")?;
        fs::write(root.join("README.md"), "readme")?;
        fs::create_dir_all(root.join(".git"))?;
        fs::write(root.join(".git/config"), "config")?;
        fs::create_dir_all(root.join("target"))?;
        fs::write(root.join("target/debug"), "binary")?;

        // Create filter file
        let filter_content = "#exclude\n.git\ntarget/\n\n#include\n*.rs\n*.md";
        let filter_dir = root.join(".ai");
        fs::create_dir(&filter_dir)?;
        fs::write(filter_dir.join("filter=all"), filter_content)?;

        let paths = vec![root.to_str().unwrap().to_string()];
        let files = get_files(&paths, Some(root))?;

        let mut expected = vec![
            root.join("README.md").to_string_lossy().into_owned(),
            root.join("src/main.rs").to_string_lossy().into_owned(),
        ];
        expected.sort();

        let mut actual = files;
        actual.sort();

        assert_eq!(actual, expected);

        Ok(())
    }

    #[test]
    fn test_get_files_no_filter_file() -> crate::error::Result<()> {
        let dir = tempdir()?;
        let root = dir.path();

        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/main.rs"), "fn main() {}")?;
        fs::create_dir_all(root.join(".git"))?;
        fs::write(root.join(".git/config"), "config")?;

        let paths = vec![root.to_str().unwrap().to_string()];
        let files = get_files(&paths, Some(root))?;

        let mut expected = vec![root.join("src/main.rs").to_string_lossy().into_owned()];
        expected.sort();

        let mut actual = files;
        actual.sort();

        assert_eq!(actual, expected);

        Ok(())
    }
}
