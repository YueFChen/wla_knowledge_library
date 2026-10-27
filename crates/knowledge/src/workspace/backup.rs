use super::*;
use std::{collections::HashSet, io::Read};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    schema: u32,
    library: Library,
    snapshot: Snapshot,
}

impl Workspace {
    pub fn export_library(&self, id: &str) -> Result<PathBuf> {
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        self.ensure_idle(id)?;
        let mut library = self.library(id, false)?;
        if let Some(profile) = library
            .model
            .as_deref()
            .and_then(|id| self.resolve_model(id).ok())
        {
            library.model_requirement = Some((&profile.profile).into());
        }
        library.model = None;
        library.vector_revision = None;
        for source in &mut library.sources {
            source.paths.clear();
        }
        let snapshot = self.snapshot(&library)?;
        let directory = self.root.join("exports");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let path = directory.join(format!("library-{}.zip", unique()));
        let mut writer =
            zip::ZipWriter::new(fs::File::create(&path).map_err(|error| error.to_string())?);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer
            .start_file("library.json", options)
            .map_err(|error| error.to_string())?;
        let data = Backup {
            schema: SCHEMA,
            library: library.clone(),
            snapshot: snapshot.clone(),
        };
        serde_json::to_writer(&mut writer, &data).map_err(|error| error.to_string())?;
        let mut keys = snapshot
            .documents
            .iter()
            .map(|document| document.raw_asset.clone())
            .collect::<HashSet<_>>();
        for document in &snapshot.documents {
            keys.extend(asset_keys(&document.elements));
        }
        for key in keys {
            if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("快照资产身份无效".into());
            }
            writer
                .start_file(format!("assets/{key}"), options)
                .map_err(|error| error.to_string())?;
            std::io::copy(
                &mut fs::File::open(self.directory(id)?.join("assets").join(&key))
                    .map_err(|error| error.to_string())?,
                &mut writer,
            )
            .map_err(|error| error.to_string())?;
        }
        writer.finish().map_err(|error| error.to_string())?;
        Ok(path)
    }

    pub fn import_library(&self, path: &Path) -> Result<Library> {
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        let mut archive =
            zip::ZipArchive::new(fs::File::open(path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        if archive.len() > 200000 {
            return Err("备份文件数量超限".into());
        }
        let bytes = {
            let mut file = archive
                .by_name("library.json")
                .map_err(|_| "不是新知识库备份")?;
            if file.size() > 128 * 1024 * 1024 {
                return Err("备份清单超过 128 MiB".into());
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            bytes
        };
        let mut backup: Backup =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if backup.schema != SCHEMA || backup.snapshot.schema != SCHEMA {
            return Err("备份格式版本不兼容".into());
        }
        if backup.library.name.len() > 256 || backup.library.sources.len() > 128 {
            return Err("备份配置超出范围".into());
        }
        let mut sources = HashSet::new();
        for source in &mut backup.library.sources {
            if !valid_id(&source.id) || !sources.insert(source.id.clone()) {
                return Err("备份资料源身份无效或重复".into());
            }
            source.paths.clear();
            source.validate()?;
        }
        let mut docs = HashSet::new();
        let mut keys = HashSet::new();
        for document in &backup.snapshot.documents {
            if !valid_id(&document.id)
                || !docs.insert(document.id.clone())
                || document.id != document.metadata.path_id
                || !sources.contains(&document.source_id)
            {
                return Err("备份文档身份或来源不一致".into());
            }
            keys.insert(document.raw_asset.clone());
            keys.extend(asset_keys(&document.elements));
        }
        let mut total = 0_u64;
        for key in &keys {
            if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("备份资产身份无效".into());
            }
            let file = archive
                .by_name(&format!("assets/{key}"))
                .map_err(|_| "备份资产缺失")?;
            if file.size() > 32 * 1024 * 1024 {
                return Err("单个资料超过 32 MiB".into());
            }
            total = total.checked_add(file.size()).ok_or("备份大小溢出")?;
            if total > 4 * 1024 * 1024 * 1024 {
                return Err("备份资料超过 4 GiB".into());
            }
        }
        let mut library = backup.library;
        library.id = unique();
        library.name = format!("{}（导入）", library.name);
        library.exposed = false;
        library.model = None;
        library.snapshot = None;
        library.vector_revision = None;
        library.created_at = now();
        library.updated_at = now();
        let result = (|| {
            for key in keys {
                let mut file = archive
                    .by_name(&format!("assets/{key}"))
                    .map_err(|error| error.to_string())?;
                let mut bytes = Vec::new();
                (&mut file)
                    .take(32 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                if models::digest(&bytes) != key {
                    return Err("备份资产 SHA-256 校验失败".into());
                }
                self.store_asset(&library, &bytes)?;
            }
            self.publish(&mut library, &backup.snapshot)?;
            let mut registry = self.registry()?;
            registry.libraries.push(library.clone());
            self.save(&registry)?;
            Ok(library.clone())
        })();
        if result.is_err()
            && let Ok(directory) = self.directory(&library.id)
        {
            let _ = fs::remove_dir_all(directory);
        }
        result
    }
}
