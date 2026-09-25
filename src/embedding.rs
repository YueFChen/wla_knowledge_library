//! Shared local-model preference and inventory for the desktop UI and MCP entry.

use std::fs;
use std::path::Path;

use wonderland_plugin_sdk::PluginFailure;
use wonderland_knowledge::{LOCAL_EMBEDDING_MODEL_DIR, LOCAL_EMBEDDING_MODEL_FILES};

use crate::KnowledgeModelInfo;

pub(crate) fn model_inventory(cache_dir: &Path) -> Result<Vec<KnowledgeModelInfo>, PluginFailure> {
    let root = cache_dir.join("models");
    let preset = root.join(LOCAL_EMBEDDING_MODEL_DIR);
    let missing_files = LOCAL_EMBEDDING_MODEL_FILES
        .iter()
        .filter(|name| !preset.join(name).is_file())
        .map(|name| (*name).to_owned())
        .collect();
    let mut models = vec![KnowledgeModelInfo {
        name: LOCAL_EMBEDDING_MODEL_DIR.to_owned(),
        path: preset.display().to_string(),
        supported: true,
        missing_files,
    }];
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(models),
        Err(error) => {
            return Err(PluginFailure::LocalData(format!(
                "读取本地模型目录失败：{error}"
            )));
        }
    };
    for entry in entries {
        let entry =
            entry.map_err(|error| PluginFailure::LocalData(format!("读取模型条目失败：{error}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == LOCAL_EMBEDDING_MODEL_DIR || !entry.path().is_dir() {
            continue;
        }
        models.push(KnowledgeModelInfo {
            name,
            path: entry.path().display().to_string(),
            supported: false,
            missing_files: vec![],
        });
    }
    models[1..].sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}
