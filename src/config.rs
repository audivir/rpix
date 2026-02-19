use crate::InputType;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::env;

#[cfg(not(target_os = "macos"))]
use directories::ProjectDirs;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Plugin {
    pub extensions: Vec<String>,
    pub magic_bytes: Option<Vec<String>>,
    pub output: InputType,
    pub path: String,
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PluginConfig {
    #[serde(flatten)]
    pub plugins: HashMap<String, Plugin>,
}

pub struct Subdirs {
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
}

#[cfg(not(target_os = "macos"))]
pub fn kv_project_dirs() -> Subdirs {
    let project_dirs =
        ProjectDirs::from("de", "audivir", "kv").expect("Could not determine XDG directories");
    Subdirs {
        cache_dir: project_dirs.cache_dir().to_path_buf(),
        data_dir: project_dirs.data_dir().to_path_buf(),
        config_dir: project_dirs.config_dir().to_path_buf(),
    }
}

#[cfg(target_os = "macos")]
pub fn kv_project_dirs() -> Subdirs {
    let home_dir = env::home_dir().expect("Could not determine home directory");

    let cache_dir = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir.join(".cache"));

    let data_dir = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir.join(".local/share"));

    let config_dir = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir.join(".config"));

    Subdirs {
        cache_dir: cache_dir.join("kv"),
        data_dir: data_dir.join("kv"),
        config_dir: config_dir.join("kv"),
    }
}

pub fn get_config_path() -> PathBuf {
    kv_project_dirs().config_dir.join("plugins.toml")
}

pub fn load_plugins() -> HashMap<String, Plugin> {
    let config_path = get_config_path();

    if !config_path.exists() {
        return HashMap::new();
    }

    match std::fs::read_to_string(&config_path) {
        Ok(content) => match toml::from_str::<PluginConfig>(&content) {
            Ok(cfg) => cfg.plugins,
            Err(e) => {
                eprintln!("Warning: Failed to parse plugins.toml: {}", e);
                HashMap::new()
            }
        },
        Err(e) => {
            eprintln!("Warning: Failed to read plugins.toml: {}", e);
            HashMap::new()
        }
    }
}

pub fn has_extension_or_magic_bytes(
    data: &[u8],
    extension: &str,
    magic_hex_list: &[String],
    extensions: &[String],
) -> bool {
    for hex_str in magic_hex_list {
        // convert hex string to byte vector
        if let Ok(magic) = hex::decode(hex_str) {
            if data.len() >= magic.len() && &data[0..magic.len()] == magic.as_slice() {
                return true;
            }
        }
    }
    if extensions.contains(&extension.to_string()) {
        return true;
    }
    false
}

pub fn open_config() -> Result<()> {
    let path = get_config_path();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create configuration directory")?;
    }

    if !path.exists() {
        let template = r#"# KV plugins configuration
#
# Define custom handlers for file extensions.
#
# Example: Convert .xml files to SVG using a CLI tool
# [xml-converter]
# extensions = ["xml"]
# output = "svg"
# path = "convert-xml"
# placeholder = "{}" # Optional: if omitted, input is piped to stdin
#
# Example: Handle binary files with magic bytes
# [custom-binary]
# extensions = ["bin"]
# magic_bytes = ["CAFEBABE"]
# output = "png"
# path = "my-converter"
"#;
        std::fs::write(&path, template).context("Failed to create plugins.toml")?;
        eprintln!("Created default config file at: {}", path.display());
    }

    println!("{}", path.display());

    Ok(())
}
