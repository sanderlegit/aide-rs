use crate::error::{Error, Result};
use rustdoc_json::Builder;
use rustdoc_types::{Crate, Id, Item, ItemEnum, Module, Type};
use serde_json::json;
use std::path::{Path, PathBuf};

fn generate_docs(crate_name: &str, current_dir: Option<&Path>) -> Result<PathBuf> {
    // On some platforms (like macOS), temp directories can be under symlinked paths.
    // Canonicalizing the path ensures that cargo can find its working directory.
    let canonical_dir = current_dir.map(|p| p.canonicalize()).transpose()?;

    let manifest_path = canonical_dir
        .as_deref()
        .map(|d| d.join("Cargo.toml"))
        .unwrap_or_else(|| PathBuf::from("Cargo.toml"));

    let mut builder = Builder::default()
        .toolchain("nightly")
        .package(crate_name)
        .manifest_path(&manifest_path)
        .quiet(true);

    if let Some(dir) = &canonical_dir {
        builder = builder.target_dir(dir.join("target"));
    }

    let build_result = builder.build();

    build_result.map_err(|e| {
        Error::Config(format!(
            "Failed to build rustdoc for {}: {}",
            crate_name, e
        ))
    })
}

fn get_crate_docs_from_krate(krate: &Crate, crate_name: &str) -> Result<serde_json::Value> {
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
        "version": krate.crate_version.clone().unwrap_or_default(),
        "documentation": root_module.docs.clone().unwrap_or_default(),
        "modules": modules,
    }))
}

pub fn get_crate_docs(
    crate_name: &str,
    current_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    let json_path = generate_docs(crate_name, current_dir)?;
    let krate: Crate = serde_json::from_reader(std::fs::File::open(json_path)?)?;
    get_crate_docs_from_krate(&krate, crate_name)
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

fn get_module_docs_json(
    krate: &Crate,
    item: &Item,
    crate_name: &str,
    path: &str,
) -> Result<serde_json::Value> {
    if let ItemEnum::Module(module) = &item.inner {
        let (structs, enums, functions) = get_module_item_names(krate, module);
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
        Err(Error::Config("Item is not a module".to_string()))
    }
}

pub fn get_item_docs(
    crate_name: &str,
    path: &str,
    current_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    let json_path = generate_docs(crate_name, current_dir)?;
    let krate: Crate = serde_json::from_reader(std::fs::File::open(json_path)?)?;

    match find_item_by_path(&krate, path) {
        Ok(item) => match &item.inner {
            ItemEnum::Module(_) => get_module_docs_json(&krate, item, crate_name, path),
            ItemEnum::Struct(s) => {
                let (methods, impls) = get_methods_and_trait_impls(&krate, &s.impls);
                Ok(json!({
                    "type": "struct",
                    "crate": crate_name,
                    "path": path,
                    "documentation": item.docs.clone().unwrap_or_default(),
                    "methods": methods,
                    "trait_implementations": impls,
                }))
            }
            ItemEnum::Enum(e) => {
                let (methods, impls) = get_methods_and_trait_impls(&krate, &e.impls);
                Ok(json!({
                    "type": "enum",
                    "crate": crate_name,
                    "path": path,
                    "documentation": item.docs.clone().unwrap_or_default(),
                    "methods": methods,
                    "trait_implementations": impls,
                }))
            }
            ItemEnum::Function(func) => Ok(json!({
                "type": "function",
                "crate": crate_name,
                "path": path,
                "documentation": item.docs.clone().unwrap_or_default(),
                "signature": clean_fn_signature(&func.sig.output, &func.sig.inputs, item.name.as_deref().unwrap_or("")),
            })),
            _ => Err(Error::Config(format!(
                "Path '{}' is not a module, struct, enum, or function.",
                path
            ))),
        },
        Err(_) => {
            // The requested path was not found. Try to find a parent module.
            let mut path_parts: Vec<&str> = path.split("::").collect();

            while path_parts.len() > 1 {
                path_parts.pop();
                let parent_path = path_parts.join("::");
                if let Ok(item) = find_item_by_path(&krate, &parent_path) {
                    if let ItemEnum::Module(_) = &item.inner {
                        let mut module_docs =
                            get_module_docs_json(&krate, item, crate_name, &parent_path)?;
                        let note = format!(
                            "Note: The original path '{}' was not found. Returning docs for the parent module '{}'.",
                            path, parent_path
                        );
                        if let Some(obj) = module_docs.as_object_mut() {
                            obj.insert("note".to_string(), json!(note));
                        }
                        return Ok(module_docs);
                    }
                }
            }

            // If no parent module was found, fall back to crate-level documentation.
            let mut crate_docs = get_crate_docs_from_krate(&krate, crate_name)?;
            let note = format!(
                "Note: The path '{}' was not found. Returning crate-level documentation.",
                path
            );
            if let Some(obj) = crate_docs.as_object_mut() {
                obj.insert("note".to_string(), json!(note));
            }
            Ok(crate_docs)
        }
    }
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

fn get_methods_and_trait_impls(
    krate: &Crate,
    impl_ids: &[Id],
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut inherent_methods = Vec::new();
    let mut trait_impls = Vec::new();

    for impl_id in impl_ids {
        if let Some(impl_item) = krate.index.get(impl_id) {
            if let ItemEnum::Impl(imp) = &impl_item.inner {
                let mut methods_in_impl = Vec::new();
                for item_id in &imp.items {
                    if let Some(method_item) = krate.index.get(item_id) {
                        if let ItemEnum::Function(func) = &method_item.inner {
                            methods_in_impl.push(json!({
                                "name": method_item.name.clone().unwrap_or_default(),
                                "signature": clean_fn_signature(&func.sig.output, &func.sig.inputs, method_item.name.as_deref().unwrap_or("")),
                                "documentation": method_item.docs.clone().unwrap_or_default(),
                            }));
                        }
                    }
                }

                if let Some(trait_path) = &imp.trait_ {
                    // Only add trait impl if it has methods.
                    if !methods_in_impl.is_empty() {
                        trait_impls.push(json!({
                            "trait": trait_path.path.clone(),
                            "methods": methods_in_impl,
                        }));
                    }
                } else {
                    inherent_methods.append(&mut methods_in_impl);
                }
            }
        }
    }
    (inherent_methods, trait_impls)
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
    use std::path::PathBuf;
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

        // Add a .cargo/config.toml to explicitly set the target directory
        // for this temporary crate, preventing interference from the parent
        // project's workspace and target directory.
        let cargo_config_dir = crate_root.join(".cargo");
        fs::create_dir(&cargo_config_dir).unwrap();
        fs::write(
            cargo_config_dir.join("config.toml"),
            "[build]\ntarget-dir = \"target\"\n",
        )
        .unwrap();

        let lib_rs = r#"
//! Crate documentation.

/// A free function.
pub fn a_free_function() {}

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
        let result = get_crate_docs("test_crate", Some(&crate_root)).unwrap();

        assert_eq!(result["type"], "crate");
        assert_eq!(result["name"], "test_crate");
        assert_eq!(result["version"], "0.1.0");
        assert_eq!(result["documentation"], "Crate documentation.");
        assert_eq!(result["modules"], json!(["my_module"]));
    }

    #[test]
    fn test_get_item_docs() {
        let (_dir, crate_root) = setup_test_crate();

        // Test getting module docs
        let module_result =
            get_item_docs("test_crate", "test_crate::my_module", Some(&crate_root)).unwrap();

        assert_eq!(module_result["type"], "module");
        assert_eq!(module_result["crate"], "test_crate");
        assert_eq!(module_result["path"], "test_crate::my_module");
        assert_eq!(module_result["documentation"], "Module documentation.");
        assert_eq!(module_result["structs"], json!(["MyStruct"]));
        assert_eq!(module_result["enums"], json!(["MyEnum"]));
        assert!(module_result["functions"].as_array().unwrap().is_empty());

        // Test getting struct docs
        let struct_result = get_item_docs(
            "test_crate",
            "test_crate::my_module::MyStruct",
            Some(&crate_root),
        )
        .unwrap();

        assert_eq!(struct_result["type"], "struct");
        assert_eq!(struct_result["crate"], "test_crate");
        assert_eq!(struct_result["path"], "test_crate::my_module::MyStruct");
        assert_eq!(struct_result["documentation"], "Struct documentation.");
        let methods = struct_result["methods"].as_array().unwrap();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0]["name"], "new");
        assert_eq!(methods[0]["documentation"], "A method on MyStruct.");
        assert_eq!(methods[0]["signature"], "pub fn new() -> Self");

        // Test getting function docs
        let function_result =
            get_item_docs("test_crate", "test_crate::a_free_function", Some(&crate_root)).unwrap();
        assert_eq!(function_result["type"], "function");
        assert_eq!(function_result["path"], "test_crate::a_free_function");
        assert_eq!(function_result["documentation"], "A free function.");
        assert_eq!(
            function_result["signature"],
            "pub fn a_free_function()"
        );
    }
}
