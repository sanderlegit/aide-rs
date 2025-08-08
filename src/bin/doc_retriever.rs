use aide_rs::error::{Error, Result};
use clap::{Parser, Subcommand};
use rustdoc_json::Builder;
use rustdoc_types::{Crate, Id, Item, ItemEnum, Module, Struct, Type};
use serde_json::json;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "doc-retriever",
    about = "A tool to retrieve Rust documentation as structured JSON."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Get crate-level documentation.
    Crate {
        #[arg(long)]
        name: String,
    },
    /// Get module-level documentation.
    Module {
        #[arg(long = "crate")]
        crate_name: String,
        #[arg(long)]
        path: String,
    },
    /// Get type-level documentation (struct or enum).
    Type {
        #[arg(long = "crate")]
        crate_name: String,
        #[arg(long)]
        path: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Crate { name } => get_crate_docs(&name),
        Commands::Module { crate_name, path } => get_module_docs(&crate_name, &path),
        Commands::Type { crate_name, path } => get_type_docs(&crate_name, &path),
    };

    match result {
        Ok(json) => {
            println!("{}", serde_json::to_string_pretty(&json)?);
            Ok(())
        }
        Err(e) => {
            let json_err = json!({
                "success": false,
                "error": e.to_string(),
            });
            eprintln!("{}", serde_json::to_string_pretty(&json_err)?);
            std::process::exit(1);
        }
    }
}

fn generate_docs(crate_name: &str) -> Result<PathBuf> {
    Builder::default()
        .package(crate_name)
        .quiet(true)
        .build()
        .map_err(|e| Error::Config(format!("Failed to build rustdoc for {}: {}", crate_name, e)))
}

fn get_crate_docs(crate_name: &str) -> Result<serde_json::Value> {
    let json_path = generate_docs(crate_name)?;
    let krate: Crate = serde_json::from_reader(std::fs::File::open(json_path)?)?;

    let root_module = krate
        .index
        .get(&krate.root)
        .ok_or_else(|| Error::Config("Root module not found in crate".to_string()))?;

    let modules = if let ItemEnum::Module(m) = &root_module.inner {
        m.items
            .iter()
            .filter_map(|id| {
                if let Some(item) = krate.index.get(id) {
                    if matches!(item.inner, ItemEnum::Module(_)) {
                        return item.name.clone();
                    }
                }
                None
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    Ok(json!({
        "type": "crate",
        "name": crate_name,
        "version": krate.crate_version.unwrap_or_default(),
        "documentation": root_module.docs.clone().unwrap_or_default(),
        "modules": modules,
    }))
}

fn find_item_by_path<'a>(krate: &'a Crate, path: &str) -> Result<&'a Item> {
    let path_parts: Vec<&str> = path.split("::").collect();
    krate
        .paths
        .iter()
        .find(|(_, summary)| summary.path == path_parts)
        .and_then(|(id, _)| krate.index.get(id))
        .ok_or_else(|| Error::Config(format!("Path not found in crate: {}", path)))
}

fn get_module_docs(crate_name: &str, path: &str) -> Result<serde_json::Value> {
    let json_path = generate_docs(crate_name)?;
    let krate: Crate = serde_json::from_reader(std::fs::File::open(json_path)?)?;

    let item = find_item_by_path(&krate, path)?;
    if let ItemEnum::Module(module) = &item.inner {
        let (structs, enums, functions) = get_module_item_names(&krate, module);
        Ok(json!({
            "type": "module",
            "crate": crate_name,
            "path": path,
            "documentation": item.docs.clone().unwrap_or_default(),
            "structs": structs,
            "enums": enums,
            "functions": functions,
        }))
    } else {
        Err(Error::Config(format!("Path is not a module: {}", path)))
    }
}

fn get_type_docs(crate_name: &str, path: &str) -> Result<serde_json::Value> {
    let json_path = generate_docs(crate_name)?;
    let krate: Crate = serde_json::from_reader(std::fs::File::open(json_path)?)?;

    let item = find_item_by_path(&krate, path)?;
    let (type_name, methods, impls) = match &item.inner {
        ItemEnum::Struct(s) => ("struct", get_methods(&krate, s), get_impls(&krate, &item.id)),
        ItemEnum::Enum(e) => ("enum", get_methods(&krate, e), get_impls(&krate, &item.id)),
        _ => {
            return Err(Error::Config(format!(
                "Path is not a struct or enum: {}",
                path
            )))
        }
    };

    Ok(json!({
        "type": type_name,
        "crate": crate_name,
        "path": path,
        "documentation": item.docs.clone().unwrap_or_default(),
        "methods": methods,
        "trait_implementations": impls,
    }))
}

fn get_module_item_names(krate: &Crate, module: &Module) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();

    for id in &module.items {
        if let Some(item) = krate.index.get(id) {
            if let Some(name) = &item.name {
                match item.inner {
                    ItemEnum::Struct(_) => structs.push(name.clone()),
                    ItemEnum::Enum(_) => enums.push(name.clone()),
                    ItemEnum::Function(_) => functions.push(name.clone()),
                    _ => {}
                }
            }
        }
    }
    (structs, enums, functions)
}

trait HasItems {
    fn get_items(&self) -> &Vec<Id>;
}
impl HasItems for Struct {
    fn get_items(&self) -> &Vec<Id> {
        &self.impls
    }
}
impl HasItems for rustdoc_types::Enum {
    fn get_items(&self) -> &Vec<Id> {
        &self.impls
    }
}

fn get_methods<T: HasItems>(krate: &Crate, item_with_impls: &T) -> Vec<serde_json::Value> {
    let mut methods = Vec::new();
    for impl_id in item_with_impls.get_items() {
        if let Some(impl_item) = krate.index.get(impl_id) {
            if let ItemEnum::Impl(imp) = &impl_item.inner {
                for item_id in &imp.items {
                    if let Some(method_item) = krate.index.get(item_id) {
                        if let ItemEnum::Function(func) = &method_item.inner {
                            methods.push(json!({
                                "name": method_item.name.clone().unwrap_or_default(),
                                "signature": clean_fn_signature(&func.sig.output, &func.sig.inputs, method_item.name.as_deref().unwrap_or("")),
                                "documentation": method_item.docs.clone().unwrap_or_default(),
                            }));
                        }
                    }
                }
            }
        }
    }
    methods
}

fn get_impls(krate: &Crate, item_id: &Id) -> Vec<String> {
    krate
        .index
        .values()
        .filter_map(|item| match &item.inner {
            ItemEnum::Impl(imp) => {
                if let Type::ResolvedPath(path) = &imp.for_ {
                    if &path.id == item_id {
                        return imp.trait_.as_ref().map(|t| t.path.clone());
                    }
                }
                None
            }
            _ => None,
        })
        .collect()
}

fn clean_fn_signature(
    output: &Option<Type>,
    inputs: &[(String, Type)],
    name: &str,
) -> String {
    let args = inputs
        .iter()
        .map(|(name, ty)| format!("{}: {}", name, clean_type(ty)))
        .collect::<Vec<_>>()
        .join(", ");

    let ret = output
        .as_ref()
        .map(|ty| format!(" -> {}", clean_type(ty)))
        .unwrap_or_default();

    format!("pub fn {}({}){}", name, args, ret)
}

fn clean_type(ty: &Type) -> String {
    match ty {
        Type::ResolvedPath(path) => path.path.clone(),
        Type::Generic(name) => name.clone(),
        Type::Primitive(name) => name.clone(),
        Type::BorrowedRef { type_, .. } => format!("&{}", clean_type(type_)),
        _ => "impl ...".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn setup_test_crate() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let crate_root = dir.path().join("test_crate");
        fs::create_dir_all(crate_root.join("src")).unwrap();

        let cargo_toml = r#"
[package]
name = "test_crate"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(crate_root.join("Cargo.toml"), cargo_toml).unwrap();

        let lib_rs = r#"
//! Crate documentation.

pub mod my_module {
    //! Module documentation.

    /// Struct documentation.
    pub struct MyStruct {
        pub field: u32,
    }

    impl MyStruct {
        /// A method on MyStruct.
        pub fn new() -> Self {
            Self { field: 0 }
        }
    }

    /// Enum documentation.
    pub enum MyEnum {
        VariantA,
    }
}
"#;
        fs::write(crate_root.join("src/lib.rs"), lib_rs).unwrap();

        (dir, crate_root)
    }

    #[test]
    fn test_get_crate_docs() {
        let (_dir, crate_root) = setup_test_crate();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&crate_root).unwrap();

        let result = get_crate_docs("test_crate").unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(result["type"], "crate");
        assert_eq!(result["name"], "test_crate");
        assert_eq!(result["version"], "0.1.0");
        assert_eq!(result["documentation"], "Crate documentation.");
        assert_eq!(result["modules"], json!(["my_module"]));
    }

    #[test]
    fn test_get_module_docs() {
        let (_dir, crate_root) = setup_test_crate();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&crate_root).unwrap();

        let result = get_module_docs("test_crate", "test_crate::my_module").unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(result["type"], "module");
        assert_eq!(result["crate"], "test_crate");
        assert_eq!(result["path"], "test_crate::my_module");
        assert_eq!(result["documentation"], "Module documentation.");
        assert_eq!(result["structs"], json!(["MyStruct"]));
        assert_eq!(result["enums"], json!(["MyEnum"]));
        assert!(result["functions"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_get_type_docs_struct() {
        let (_dir, crate_root) = setup_test_crate();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&crate_root).unwrap();

        let result = get_type_docs("test_crate", "test_crate::my_module::MyStruct").unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(result["type"], "struct");
        assert_eq!(result["crate"], "test_crate");
        assert_eq!(result["path"], "test_crate::my_module::MyStruct");
        assert_eq!(result["documentation"], "Struct documentation.");
        let methods = result["methods"].as_array().unwrap();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0]["name"], "new");
        assert_eq!(methods[0]["documentation"], "A method on MyStruct.");
    }
}
