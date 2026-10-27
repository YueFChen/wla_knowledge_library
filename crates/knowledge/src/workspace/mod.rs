//! Independent libraries, managed snapshots and read-only routing shared by Core and MCP.
mod backup;
mod ingest;
mod retrieval;
mod services;
pub use ingest::image_mime;
pub use retrieval::{LibraryHit, SearchResult};
#[cfg(test)]
mod tests;

use crate::{
    Doc, KnowledgeBase, PageElement,
    models::{self, ModelPool},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, String>;
pub const SCHEMA: u32 = 1;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn unique() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}-{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'))
}
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}
pub fn atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    fs::create_dir_all(path.parent().ok_or("无效数据路径")?).map_err(|error| error.to_string())?;
    let temp = path.with_extension(format!("tmp-{}", unique()));
    let result = (|| {
        let mut file = fs::File::create(&temp).map_err(|error| error.to_string())?;
        serde_json::to_writer(&mut file, value).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temp, path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub path_prefix: String,
    #[serde(default = "default_depth")]
    pub depth: usize,
    #[serde(default = "default_pages")]
    pub max_pages: usize,
    #[serde(default)]
    pub dynamic: bool,
}
fn default_depth() -> usize {
    2
}
fn default_pages() -> usize {
    100
}
impl Source {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || self.name.len() > 256
            || self.paths.len() > 1024
            || self.depth > 10
            || self.max_pages == 0
            || self.max_pages > 10000
        {
            return Err("资料源配置超出范围".into());
        }
        match self.kind.as_str() {
            "files" | "directory" => {
                if self.paths.iter().any(|path| !Path::new(path).is_absolute())
                    || (self.kind == "directory" && self.paths.len() > 1)
                {
                    return Err("请选择本机绝对路径".into());
                }
            }
            "website" => {
                let url = url::Url::parse(&self.url).map_err(|_| "网页地址无效")?;
                if !matches!(url.scheme(), "http" | "https")
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err("请使用无需登录的 HTTP(S) 网页地址".into());
                }
                if !self.path_prefix.is_empty() && !self.path_prefix.starts_with('/') {
                    return Err("网页路径范围必须以 / 开头".into());
                }
            }
            "wonderland" => {}
            _ => return Err("不支持的资料源类型".into()),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub sources: Vec<Source>,
}

pub fn builtins() -> Vec<Preset> {
    let source = Source {
        id: String::new(),
        name: "千星奇域官方资料".into(),
        kind: "wonderland".into(),
        paths: vec![],
        url: String::new(),
        path_prefix: String::new(),
        depth: 2,
        max_pages: 10000,
        dynamic: true,
    };
    let wonderland = Preset {
        id: "wonderland".into(),
        name: "千星奇域官方资料".into(),
        sources: vec![source.clone()],
    };
    vec![
        Preset {
            id: "blank".into(),
            name: "空白知识库".into(),
            sources: vec![],
        },
        wonderland,
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub id: String,
    pub name: String,
    pub preset_id: String,
    pub sources: Vec<Source>,
    pub exposed: bool,
    pub model: Option<String>,
    #[serde(default)]
    pub model_requirement: Option<models::ModelRequirement>,
    pub revision: String,
    pub snapshot: Option<String>,
    pub vector_revision: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema: u32,
    pub libraries: Vec<Library>,
    pub presets: Vec<Preset>,
    #[serde(default)]
    pub services: Vec<models::Service>,
    #[serde(default)]
    pub models: Vec<models::ModelProfile>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            libraries: vec![],
            presets: vec![],
            services: vec![],
            models: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub source_id: String,
    pub source_key: String,
    pub title: String,
    pub url: String,
    pub fingerprint: String,
    pub raw_asset: String,
    pub updated_at: u64,
    pub elements: Vec<PageElement>,
    pub metadata: Doc,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    pub documents: Vec<Document>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            documents: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub library_id: Option<String>,
    pub state: String,
    pub done: usize,
    pub total: usize,
    pub current: String,
    pub errors: Vec<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct LogEntry {
    pub sequence: usize,
    pub time: u64,
    pub level: String,
    pub message: String,
}
#[derive(Default)]
struct TaskLog {
    id: String,
    entries: Vec<LogEntry>,
}

pub struct Resource {
    pub status: u16,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub bytes: Vec<u8>,
}
impl Resource {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}
pub trait Fetcher: Send + Sync {
    fn get(&self, url: &str) -> Result<Resource>;
    fn render(&self, url: &str, cancelled: &AtomicBool) -> Result<String>;
}

pub struct Workspace {
    root: PathBuf,
    mutation: Mutex<()>,
    pub(crate) cancelled: AtomicBool,
    job: Mutex<Job>,
    task_log: Mutex<TaskLog>,
    pub(crate) pool: ModelPool,
    pub(crate) indexes: Mutex<HashMap<String, (String, Arc<KnowledgeBase>)>>,
}

impl Workspace {
    /// Construction and every read remain side-effect free for MCP.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            mutation: Mutex::new(()),
            cancelled: AtomicBool::new(false),
            job: Mutex::new(Job::default()),
            task_log: Mutex::new(TaskLog::default()),
            pool: ModelPool::default(),
            indexes: Mutex::new(HashMap::new()),
        }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn registry(&self) -> Result<Registry> {
        let path = self.root.join("registry.json");
        if !path.exists() {
            return Ok(Registry::default());
        }
        let registry: Registry = read_json(&path)?;
        if registry.schema != SCHEMA
            || registry.libraries.iter().any(|library| {
                !valid_id(&library.id)
                    || library
                        .snapshot
                        .as_ref()
                        .is_some_and(|value| !valid_id(value))
            })
        {
            return Err("知识库数据版本或身份无效".into());
        }
        Ok(registry)
    }
    fn save(&self, registry: &Registry) -> Result<()> {
        let old = self.root.join("registry.json");
        let backup = self.root.join("registry.before-services.json");
        if old.is_file() && !backup.exists() {
            let bytes = fs::read(&old).map_err(|error| error.to_string())?;
            if serde_json::from_slice::<serde_json::Value>(&bytes)
                .is_ok_and(|value| value.get("services").is_none())
            {
                use std::io::Write;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(backup)
                    .map_err(|error| error.to_string())?;
                file.write_all(&bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|error| error.to_string())?;
            }
        }
        atomic(&self.root.join("registry.json"), registry)
    }
    pub fn library(&self, id: &str, mcp: bool) -> Result<Library> {
        self.registry()?
            .libraries
            .into_iter()
            .find(|library| library.id == id && (!mcp || library.exposed))
            .ok_or_else(|| "知识库不存在或未授权访问".into())
    }
    pub fn scope(&self, ids: &[String], mcp: bool) -> Result<Vec<Library>> {
        let mut libraries = self
            .registry()?
            .libraries
            .into_iter()
            .filter(|library| !mcp || library.exposed)
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            if ids
                .iter()
                .any(|id| !libraries.iter().any(|library| library.id == *id))
            {
                return Err("所选知识库不存在或未授权访问".into());
            }
            libraries.retain(|library| ids.contains(&library.id));
        }
        Ok(libraries)
    }
    pub(crate) fn directory(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err("无效知识库身份".into());
        }
        Ok(self.root.join("libraries").join(id))
    }
    pub fn snapshot(&self, library: &Library) -> Result<Snapshot> {
        match &library.snapshot {
            Some(id) if valid_id(id) => {
                let snapshot: Snapshot = read_json(
                    &self
                        .directory(&library.id)?
                        .join(format!("snapshot-{id}.json")),
                )?;
                if snapshot.schema != SCHEMA {
                    return Err("快照版本不兼容".into());
                }
                Ok(snapshot)
            }
            Some(_) => Err("无效快照身份".into()),
            None => Ok(Snapshot::default()),
        }
    }
    pub fn create(&self, name: &str, preset_id: &str) -> Result<Library> {
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        if name.trim().is_empty() || name.len() > 256 {
            return Err("请输入知识库名称".into());
        }
        let mut registry = self.registry()?;
        let preset = builtins()
            .into_iter()
            .chain(registry.presets.clone())
            .find(|preset| preset.id == preset_id)
            .ok_or("预设不存在")?;
        let mut sources = preset.sources;
        for source in &mut sources {
            source.id = unique();
        }
        let library = Library {
            id: unique(),
            name: name.trim().into(),
            preset_id: preset_id.into(),
            sources,
            exposed: false,
            model: None,
            model_requirement: None,
            revision: unique(),
            snapshot: None,
            vector_revision: None,
            created_at: now(),
            updated_at: now(),
        };
        registry.libraries.push(library.clone());
        self.save(&registry)?;
        Ok(library)
    }
    pub fn update(
        &self,
        id: &str,
        name: Option<&str>,
        exposed: Option<bool>,
        model: Option<Option<String>>,
    ) -> Result<Library> {
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        if name.is_some() || model.is_some() {
            self.ensure_idle(id)?;
        }
        let mut registry = self.registry()?;
        let requirement = match model.as_ref().and_then(|value| value.as_ref()) {
            Some(id) => Some(models::ModelRequirement::from(
                registry
                    .models
                    .iter()
                    .find(|profile| profile.id == *id)
                    .ok_or("模型尚未绑定，请先配置并验证本机模型")?,
            )),
            None => None,
        };
        let library = registry
            .libraries
            .iter_mut()
            .find(|library| library.id == id)
            .ok_or("知识库不存在")?;
        if let Some(name) = name {
            if name.trim().is_empty() || name.len() > 256 {
                return Err("名称无效".into());
            }
            library.name = name.trim().into();
        }
        if let Some(exposed) = exposed {
            library.exposed = exposed;
        }
        if let Some(model) = model {
            if model != library.model {
                library.vector_revision = None;
            }
            library.model = model;
            library.model_requirement = requirement;
        }
        library.revision = unique();
        library.updated_at = now();
        let result = library.clone();
        self.save(&registry)?;
        Ok(result)
    }
    pub fn save_source(&self, id: &str, mut source: Source) -> Result<Library> {
        source.validate()?;
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        self.ensure_idle(id)?;
        let mut registry = self.registry()?;
        let library = registry
            .libraries
            .iter_mut()
            .find(|library| library.id == id)
            .ok_or("知识库不存在")?;
        if source.id.is_empty() {
            source.id = unique();
            library.sources.push(source);
        } else {
            let existing = library
                .sources
                .iter_mut()
                .find(|existing| existing.id == source.id)
                .ok_or("资料源不存在")?;
            *existing = source;
        }
        library.revision = unique();
        library.updated_at = now();
        let result = library.clone();
        self.save(&registry)?;
        Ok(result)
    }
    pub fn delete(&self, id: &str, source_id: Option<&str>, confirmed: bool) -> Result<()> {
        if !confirmed {
            return Err("请确认要永久删除的范围".into());
        }
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        self.ensure_idle(id)?;
        let mut registry = self.registry()?;
        let position = registry
            .libraries
            .iter()
            .position(|library| library.id == id)
            .ok_or("知识库不存在")?;
        if let Some(source_id) = source_id {
            let library = &mut registry.libraries[position];
            if !library.sources.iter().any(|source| source.id == source_id) {
                return Err("资料源不存在".into());
            }
            let mut snapshot = self.snapshot(library)?;
            snapshot
                .documents
                .retain(|document| document.source_id != source_id);
            library.sources.retain(|source| source.id != source_id);
            self.publish(library, &snapshot)?;
            self.save(&registry)?;
            let vectors = self.directory(id)?.join("vectors.json");
            if vectors.exists() {
                fs::remove_file(vectors).map_err(|error| error.to_string())?;
            }
            self.collect_assets(&registry.libraries[position], &snapshot)?;
        } else {
            registry.libraries.remove(position);
            self.save(&registry)?;
            let directory = self.directory(id)?;
            if directory.is_dir() {
                fs::remove_dir_all(directory)
                    .map_err(|error| format!("已撤销知识库登记，文件清理失败：{error}"))?;
            }
        }
        self.indexes
            .lock()
            .map_err(|_| "索引状态不可用")?
            .remove(id);
        Ok(())
    }
    pub fn job(&self) -> Job {
        self.job.lock().map(|job| job.clone()).unwrap_or_default()
    }
    pub fn cancel(&self) {
        if self.job().state == "running" && !self.cancelled.swap(true, Ordering::AcqRel) {
            self.log("warning", "已请求取消，正在停止当前步骤");
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub(crate) fn check_cancel(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("任务已取消".into())
        } else {
            Ok(())
        }
    }
    fn ensure_idle(&self, id: &str) -> Result<()> {
        let job = self.job();
        if job.state == "running"
            && (job.library_id.as_deref() == Some(id) || job.library_id.is_none())
        {
            Err("请先取消正在进行的任务".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn begin(&self, kind: &str, library: Option<&str>) -> Result<()> {
        let mut job = self.job.lock().map_err(|_| "任务状态不可用")?;
        if job.state == "running" {
            return Err("已有任务正在执行".into());
        }
        self.cancelled.store(false, Ordering::Release);
        *job = Job {
            id: unique(),
            kind: kind.into(),
            library_id: library.map(str::to_owned),
            state: "running".into(),
            started_at: now(),
            ..Job::default()
        };
        let mut log = self.task_log.lock().map_err(|_| "日志状态不可用")?;
        *log = TaskLog {
            id: job.id.clone(),
            entries: vec![LogEntry {
                sequence: 0,
                time: now(),
                level: "info".into(),
                message: format!("开始任务：{kind}"),
            }],
        };
        Ok(())
    }
    pub(crate) fn progress(&self, current: &str, done: usize, total: usize) {
        if let Ok(mut job) = self.job.lock() {
            job.current = current.into();
            job.done = done;
            job.total = total;
        }
        self.log("info", &format!("{current} · {done}/{total}"));
    }
    pub(crate) fn warning(&self, error: String) {
        if let Ok(mut job) = self.job.lock()
            && job.errors.len() < 100
        {
            job.errors.push(error.clone());
        }
        self.log("warning", &error);
    }
    pub(crate) fn finish(&self, result: &Result<()>) {
        if let Ok(mut job) = self.job.lock() {
            job.state = if self.cancelled.load(Ordering::Acquire) {
                "cancelled"
            } else if result.is_err() {
                "failed"
            } else {
                "completed"
            }
            .into();
            if let Err(error) = result {
                job.errors.push(error.clone());
            }
            job.ended_at = Some(now());
        }
        self.log(
            if result.is_err() { "error" } else { "info" },
            &match result {
                Err(error) => format!("任务结束：{error}"),
                Ok(()) => "任务已完成".into(),
            },
        );
    }
    pub fn log(&self, level: &str, message: &str) {
        if let Ok(mut log) = self.task_log.lock()
            && !log.id.is_empty()
        {
            let entry = LogEntry {
                sequence: log.entries.len(),
                time: now(),
                level: level.into(),
                message: message.chars().take(4096).collect(),
            };
            log.entries.push(entry);
        }
    }
    pub fn task_logs(&self, id: &str, cursor: usize) -> Result<serde_json::Value> {
        let log = self.task_log.lock().map_err(|_| "日志状态不可用")?;
        let reset = log.id != id || cursor > log.entries.len();
        let start = if reset { 0 } else { cursor };
        let end = (start + 400).min(log.entries.len());
        Ok(
            serde_json::json!({"job_id":log.id,"entries":log.entries[start..end],"next_cursor":end,"has_more":end<log.entries.len(),"reset":reset}),
        )
    }
    pub(crate) fn publish(&self, library: &mut Library, snapshot: &Snapshot) -> Result<()> {
        let revision = unique();
        atomic(
            &self
                .directory(&library.id)?
                .join(format!("snapshot-{revision}.json")),
            snapshot,
        )?;
        library.snapshot = Some(revision);
        library.revision = unique();
        library.vector_revision = None;
        library.updated_at = now();
        Ok(())
    }
    pub fn asset(&self, library_id: &str, key: &str, mcp: bool) -> Result<Vec<u8>> {
        let library = self.library(library_id, mcp)?;
        if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("无效资料身份".into());
        }
        let snapshot = self.snapshot(&library)?;
        if !snapshot.documents.iter().any(|document| {
            document.raw_asset == key
                || asset_keys(&document.elements)
                    .iter()
                    .any(|value| value == key)
        }) {
            return Err("资料未被当前快照引用".into());
        }
        fs::read(self.directory(library_id)?.join("assets").join(key))
            .map_err(|error| error.to_string())
    }
    pub fn save_preset(&self, mut preset: Preset) -> Result<Preset> {
        if preset.name.trim().is_empty() || preset.name.len() > 256 || preset.sources.len() > 128 {
            return Err("预设配置无效".into());
        }
        for source in &mut preset.sources {
            source.validate()?;
            source.id.clear();
            source.paths.clear();
        }
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        preset.id = unique();
        let mut registry = self.registry()?;
        registry.presets.push(preset.clone());
        self.save(&registry)?;
        Ok(preset)
    }
}

pub(crate) fn asset_keys(elements: &[PageElement]) -> Vec<String> {
    let mut keys = Vec::new();
    for element in elements {
        match element {
            PageElement::Image { src, .. } => {
                if let Some(key) = src.strip_prefix("asset:") {
                    keys.push(key.into());
                }
            }
            PageElement::Table { rows, .. } => {
                for cell in rows.iter().flatten() {
                    for src in &cell.images {
                        if let Some(key) = src.strip_prefix("asset:") {
                            keys.push(key.into());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    keys
}
