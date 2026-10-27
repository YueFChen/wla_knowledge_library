use super::*;
use crate::{CatalogNode, DEFAULT_LANG, KNOWLEDGE_BASE_URL, build_from_json, parsing};
use std::collections::{HashSet, VecDeque};
use std::io::Read;
use url::Url;

const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;

impl Workspace {
    pub fn build(&self, id: &str, fetcher: &dyn Fetcher, semantic: bool) -> Result<Job> {
        self.build_with(id, fetcher, semantic, &models::NativeTransport(&self.root))
    }
    pub fn build_with(
        &self,
        id: &str,
        fetcher: &dyn Fetcher,
        semantic: bool,
        transport: &dyn models::Transport,
    ) -> Result<Job> {
        let library = {
            let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
            let library = self.library(id, false)?;
            self.begin(if semantic { "semantic" } else { "update" }, Some(id))?;
            library
        };
        let result = (|| {
            if !semantic {
                let mut snapshot = self.snapshot(&library)?;
                for source in &library.sources {
                    self.check_cancel()?;
                    let old = snapshot
                        .documents
                        .iter()
                        .filter(|document| document.source_id == source.id)
                        .cloned()
                        .collect::<Vec<_>>();
                    match self.collect(&library, source, &old, fetcher) {
                        Ok((documents, complete)) => {
                            let seen = documents
                                .iter()
                                .map(|document| document.id.clone())
                                .collect::<HashSet<_>>();
                            snapshot.documents.retain(|document| {
                                document.source_id != source.id
                                    || (!complete && !seen.contains(&document.id))
                            });
                            snapshot.documents.extend(documents);
                        }
                        Err(error) => {
                            self.check_cancel()?;
                            self.warning(format!("{}：{error}", source.name));
                        }
                    }
                }
                self.check_cancel()?;
                let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
                let mut registry = self.registry()?;
                let current = registry
                    .libraries
                    .iter_mut()
                    .find(|current| current.id == id)
                    .ok_or("知识库已删除")?;
                if current.revision != library.revision {
                    return Err("知识库配置已改变，请重新更新".into());
                }
                self.publish(current, &snapshot)?;
                let committed = current.clone();
                self.save(&registry)?;
                self.collect_assets(&committed, &snapshot)?;
                self.indexes
                    .lock()
                    .map_err(|_| "索引状态不可用")?
                    .remove(id);
            }
            let current = self.library(id, false)?;
            if semantic || current.model.is_some() {
                self.build_vectors(&current, transport)?;
            }
            Ok(())
        })();
        self.finish(&result);
        // Failed builds may have staged new immutable assets; preserve only the committed snapshot.
        if result.is_err()
            && let Ok(current) = self.library(id, false)
            && let Ok(snapshot) = self.snapshot(&current)
        {
            let _ = self.collect_assets(&current, &snapshot);
        }
        result.map(|()| self.job())
    }

    fn collect(
        &self,
        library: &Library,
        source: &Source,
        old: &[Document],
        fetcher: &dyn Fetcher,
    ) -> Result<(Vec<Document>, bool)> {
        source.validate()?;
        match source.kind.as_str() {
            "files" | "directory" => self.collect_files(library, source, old, fetcher),
            "website" => self.collect_website(library, source, old, fetcher),
            "wonderland" => self.collect_wonderland(library, source, old, fetcher),
            _ => Err("未知资料源".into()),
        }
    }

    fn collect_files(
        &self,
        library: &Library,
        source: &Source,
        old: &[Document],
        fetcher: &dyn Fetcher,
    ) -> Result<(Vec<Document>, bool)> {
        if source.paths.is_empty() {
            return Err("请重新选择资料文件或目录".into());
        }
        let mut paths = Vec::new();
        if source.kind == "directory" {
            scan(Path::new(&source.paths[0]), &mut paths)?;
        } else {
            paths.extend(source.paths.iter().map(PathBuf::from));
        }
        paths.sort();
        paths.dedup();
        let total = paths.len();
        let mut documents = Vec::new();
        let mut complete = true;
        for (index, path) in paths.iter().enumerate() {
            self.check_cancel()?;
            self.progress(&path.to_string_lossy(), index, total);
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !supported(&extension) {
                continue;
            }
            let origin = Url::from_file_path(path)
                .map_err(|_| "资料文件路径无效")?
                .to_string();
            let read = (|| {
                let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
                if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
                    return Err("文本文件超过 32 MiB 或不是普通文件".into());
                }
                let bytes = fs::read(path).map_err(|error| error.to_string())?;
                let text = parsing::decode(&bytes, None)?;
                self.parse_document(
                    library,
                    source,
                    &origin,
                    &bytes,
                    &extension,
                    &text,
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("文档"),
                    old,
                    fetcher,
                )
            })();
            match read {
                Ok(document) => documents.push(document),
                Err(error) => {
                    complete = false;
                    self.warning(format!("{}：{error}", path.display()));
                    if let Some(document) =
                        old.iter().find(|document| document.source_key == origin)
                    {
                        documents.push(document.clone());
                    }
                }
            }
        }
        self.progress("文件扫描完成", total, total);
        Ok((documents, complete))
    }

    fn collect_website(
        &self,
        library: &Library,
        source: &Source,
        old: &[Document],
        fetcher: &dyn Fetcher,
    ) -> Result<(Vec<Document>, bool)> {
        let start = normalize(&source.url)?;
        let mut queue = VecDeque::from([(start, 0)]);
        let mut visited = HashSet::new();
        let mut keys = HashSet::new();
        let mut documents = Vec::new();
        let mut complete = true;
        while let Some((url, depth)) = queue.pop_front() {
            self.check_cancel()?;
            if !visited.insert(url.clone()) {
                continue;
            }
            if visited.len() > source.max_pages {
                complete = false;
                self.warning("达到网页页数上限，本次不清理未访问文档".into());
                break;
            }
            self.progress(&url, visited.len() - 1, visited.len() + queue.len());
            let read: Result<Option<Document>> = (|| {
                let resource = fetch_follow(fetcher, &url, |url| in_scope(source, url))?;
                let canonical = normalize(&resource.url)?;
                if keys.contains(&canonical) {
                    return Ok(None);
                }
                let text = if source.dynamic {
                    fetcher.render(&canonical, &self.cancelled)?
                } else {
                    resource_text(&resource)?
                };
                let extension = if canonical.ends_with(".txt") {
                    "txt"
                } else if canonical.ends_with(".md") {
                    "md"
                } else {
                    "html"
                };
                let (_, _, links) = parsing::parse(extension, &text, &canonical, &canonical)?;
                if depth < source.depth {
                    for link in links {
                        if let Ok(link) = normalize(&link)
                            && in_scope(source, &link)
                            && crawlable(&link)
                            && !visited.contains(&link)
                        {
                            queue.push_back((link, depth + 1));
                        }
                    }
                }
                let document = self.parse_document(
                    library,
                    source,
                    &canonical,
                    text.as_bytes(),
                    extension,
                    &text,
                    &canonical,
                    old,
                    fetcher,
                )?;
                keys.insert(canonical);
                Ok(Some(document))
            })();
            match read {
                Ok(Some(document)) => documents.push(document),
                Ok(None) => {}
                Err(error) => {
                    complete = false;
                    self.warning(format!("{url}：{error}"));
                    if let Some(document) = old.iter().find(|document| document.source_key == url)
                        && keys.insert(url)
                    {
                        documents.push(document.clone());
                    }
                }
            }
        }
        Ok((documents, complete))
    }

    fn collect_wonderland(
        &self,
        library: &Library,
        source: &Source,
        old: &[Document],
        fetcher: &dyn Fetcher,
    ) -> Result<(Vec<Document>, bool)> {
        let catalog_url = format!("{KNOWLEDGE_BASE_URL}/{DEFAULT_LANG}/catalog.json");
        let resource = fetch_follow(fetcher, &catalog_url, |url| {
            url.starts_with("https://act-webstatic.mihoyo.com/")
        })?;
        let catalog = resource_text(&resource)?;
        let _: Vec<CatalogNode> =
            serde_json::from_str(&catalog).map_err(|error| format!("官方目录格式无效：{error}"))?;
        let base = build_from_json(&catalog, &[]).map_err(|error| error.to_string())?;
        let targets = base
            .docs()
            .iter()
            .filter(|document| document.is_leaf)
            .collect::<Vec<_>>();
        let mut complete = targets.len() <= source.max_pages;
        let mut documents = Vec::new();
        let targets = &targets[..targets.len().min(source.max_pages)];
        for (batch_index, batch) in targets.chunks(3).enumerate() {
            self.check_cancel()?;
            let outcomes = std::thread::scope(|scope| {
                let handles = batch
                    .iter()
                    .map(|metadata| {
                        scope.spawn(move || {
                            let existing = old
                                .iter()
                                .find(|document| document.source_key == metadata.path_id);
                            let fingerprint =
                                models::digest(format!("wonderland-v3:{metadata:?}").as_bytes());
                            if let Some(existing) =
                                existing.filter(|document| document.fingerprint == fingerprint)
                            {
                                return Ok(existing.clone());
                            }
                            let html = fetcher.render(&metadata.url, &self.cancelled)?;
                            let mut elements =
                                parsing::bounded(crate::page::elements_from_dom(&html))?;
                            if elements.is_empty() {
                                return Err("官方页面未返回正文".into());
                            }
                            let id = models::digest(
                                format!("{}:{}", source.id, metadata.path_id).as_bytes(),
                            );
                            self.localize(library, source, &mut elements, &metadata.url, fetcher)?;
                            let mut document_metadata = (**metadata).clone();
                            document_metadata.path_id = id.clone();
                            Ok(Document {
                                id,
                                source_id: source.id.clone(),
                                source_key: metadata.path_id.clone(),
                                title: metadata.title.clone(),
                                url: metadata.url.clone(),
                                fingerprint,
                                raw_asset: self.store_asset(library, html.as_bytes())?,
                                updated_at: now(),
                                elements,
                                metadata: document_metadata,
                            })
                        })
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .unwrap_or_else(|_| Err("页面采集线程异常".into()))
                    })
                    .collect::<Vec<Result<Document>>>()
            });
            for (index, (metadata, outcome)) in batch.iter().zip(outcomes).enumerate() {
                self.check_cancel()?;
                self.progress(&metadata.title, batch_index * 3 + index + 1, targets.len());
                match outcome {
                    Ok(document) => documents.push(document),
                    Err(error) => {
                        complete = false;
                        self.warning(format!("{}：{error}", metadata.title));
                        if let Some(document) = old
                            .iter()
                            .find(|document| document.source_key == metadata.path_id)
                        {
                            documents.push(document.clone());
                        }
                    }
                }
            }
        }
        Ok((documents, complete))
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_document(
        &self,
        library: &Library,
        source: &Source,
        origin: &str,
        raw: &[u8],
        format: &str,
        text: &str,
        title: &str,
        old: &[Document],
        fetcher: &dyn Fetcher,
    ) -> Result<Document> {
        let fingerprint =
            models::digest(format!("parse-v2:{}:{format}:{text}", source.name).as_bytes());
        if let Some(document) = old
            .iter()
            .find(|document| document.source_key == origin && document.fingerprint == fingerprint)
        {
            self.log("info", &format!("未改变，复用快照：{}", document.title));
            return Ok(document.clone());
        }
        self.log("info", &format!("解析正文：{title}"));
        let (title, elements, _) = parsing::parse(format, text, origin, title)?;
        let mut elements = parsing::bounded(elements)?;
        self.localize(library, source, &mut elements, origin, fetcher)?;
        let id = models::digest(format!("{}:{origin}", source.id).as_bytes());
        let metadata = Doc {
            path_id: id.clone(),
            title: title.clone(),
            section: format!("资料源：{}", source.name),
            breadcrumb: vec![source.name.clone(), title.clone()],
            is_leaf: true,
            updated_at: Some(now().to_string()),
            node_graph: None,
            side: None,
            url: origin.into(),
        };
        Ok(Document {
            id,
            source_id: source.id.clone(),
            source_key: origin.into(),
            title,
            url: origin.into(),
            fingerprint,
            raw_asset: self.store_asset(library, raw)?,
            updated_at: now(),
            elements,
            metadata,
        })
    }

    fn localize(
        &self,
        library: &Library,
        source: &Source,
        elements: &mut [PageElement],
        origin: &str,
        fetcher: &dyn Fetcher,
    ) -> Result<()> {
        let mut image_cache = HashMap::new();
        let mut save = |src: &mut String| {
            if let Some(key) = image_cache.get(src) {
                *src = format!("asset:{key}");
                return;
            }
            let original = src.clone();
            let result = (|| {
                self.check_cancel()?;
                let url = Url::parse(src).map_err(|error| error.to_string())?;
                let bytes = if url.scheme() == "file" {
                    let path = fs::canonicalize(url.to_file_path().map_err(|_| "图片路径无效")?)
                        .map_err(|error| error.to_string())?;
                    let base = if source.kind == "directory" {
                        PathBuf::from(&source.paths[0])
                    } else {
                        Url::parse(origin)
                            .map_err(|error| error.to_string())?
                            .to_file_path()
                            .map_err(|_| "来源路径无效")?
                            .parent()
                            .ok_or("来源目录无效")?
                            .to_path_buf()
                    };
                    let base = fs::canonicalize(base).map_err(|error| error.to_string())?;
                    if !path.starts_with(base) {
                        return Err("图片超出所选资料目录".into());
                    }
                    if fs::metadata(&path)
                        .map_err(|error| error.to_string())?
                        .len()
                        > MAX_DOCUMENT_BYTES
                    {
                        return Err("图片超过 32 MiB".into());
                    }
                    fs::read(path).map_err(|error| error.to_string())?
                } else {
                    fetch_follow(fetcher, src, |_| true)?.bytes
                };
                if bytes.len() > 8 * 1024 * 1024 {
                    return Err("图片超过 8 MiB，保留正文".into());
                }
                if image_mime(&bytes).is_none() {
                    return Err("图片格式不受支持".into());
                }
                self.store_asset(library, &bytes)
            })();
            match result {
                Ok(key) => {
                    image_cache.insert(original, key.clone());
                    *src = format!("asset:{key}");
                    self.log("info", "配图已保存");
                }
                Err(error) => {
                    self.warning(format!("图片 {original}：{error}"));
                    src.clear();
                }
            }
        };
        for element in elements {
            match element {
                PageElement::Image { src, .. } => save(src),
                PageElement::Table { rows, .. } => {
                    for cell in rows.iter_mut().flatten() {
                        for src in &mut cell.images {
                            save(src);
                        }
                    }
                }
                _ => {}
            }
        }
        self.check_cancel()
    }
    pub(crate) fn store_asset(&self, library: &Library, bytes: &[u8]) -> Result<String> {
        let key = models::digest(bytes);
        let directory = self.directory(&library.id)?.join("assets");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let target = directory.join(&key);
        if !target.exists() {
            let temp = directory.join(format!("{}.part", unique()));
            fs::write(&temp, bytes).map_err(|error| error.to_string())?;
            if let Err(error) = fs::rename(&temp, target) {
                let _ = fs::remove_file(temp);
                return Err(error.to_string());
            }
        }
        Ok(key)
    }
    pub(crate) fn collect_assets(&self, library: &Library, snapshot: &Snapshot) -> Result<()> {
        let directory = self.directory(&library.id)?;
        let mut keys = snapshot
            .documents
            .iter()
            .map(|document| document.raw_asset.clone())
            .collect::<HashSet<_>>();
        for document in &snapshot.documents {
            keys.extend(asset_keys(&document.elements));
        }
        let assets = directory.join("assets");
        if assets.exists() {
            for entry in fs::read_dir(&assets).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                if entry
                    .file_type()
                    .map_err(|error| error.to_string())?
                    .is_file()
                    && !keys.contains(&entry.file_name().to_string_lossy().to_string())
                {
                    fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
                }
            }
        }
        if directory.exists() {
            for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("snapshot-")
                    && name.ends_with(".json")
                    && library
                        .snapshot
                        .as_ref()
                        .is_none_or(|id| name != format!("snapshot-{id}.json"))
                {
                    fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(())
    }
}

pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}
fn supported(extension: &str) -> bool {
    matches!(extension, "md" | "markdown" | "txt" | "html" | "htm")
}
fn scan(path: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(path).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            scan(&entry.path(), paths)?;
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| supported(&extension.to_ascii_lowercase()))
        {
            paths.push(entry.path());
        }
        if paths.len() > 100000 {
            return Err("目录超过十万份文档，请缩小范围".into());
        }
    }
    Ok(())
}
fn normalize(value: &str) -> Result<String> {
    let mut url = Url::parse(value).map_err(|error| error.to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("仅支持免登录 HTTP(S) 地址".into());
    }
    url.set_fragment(None);
    Ok(url.to_string())
}
pub(crate) fn in_scope(source: &Source, value: &str) -> bool {
    let (Ok(start), Ok(url)) = (Url::parse(&source.url), Url::parse(value)) else {
        return false;
    };
    let prefix = source.path_prefix.trim_end_matches('/');
    url.origin() == start.origin()
        && (prefix.is_empty()
            || url.path() == prefix
            || url.path().starts_with(&format!("{prefix}/")))
}
fn crawlable(value: &str) -> bool {
    Url::parse(value).ok().is_some_and(|url| {
        Path::new(url.path())
            .extension()
            .and_then(|extension| extension.to_str())
            .is_none_or(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "html" | "htm" | "md" | "txt" | "php" | "asp" | "aspx"
                )
            })
    })
}
fn fetch_follow(
    fetcher: &dyn Fetcher,
    start: &str,
    allowed: impl Fn(&str) -> bool,
) -> Result<Resource> {
    let mut url = normalize(start)?;
    for _ in 0..10 {
        if !allowed(&url) {
            return Err("页面或重定向超出资料源范围".into());
        }
        let resource = fetcher.get(&url)?;
        if !allowed(&resource.url) {
            return Err("最终页面超出资料源范围".into());
        }
        if (300..400).contains(&resource.status) {
            url = parsing::resolve(&url, resource.header("location").ok_or("重定向缺少目标")?)
                .ok_or("重定向地址无效")?;
        } else if (200..300).contains(&resource.status) {
            return Ok(resource);
        } else {
            return Err(format!("HTTP {}，保留上次成功资料", resource.status));
        }
    }
    Err("重定向次数超限".into())
}
fn resource_text(resource: &Resource) -> Result<String> {
    let bytes = match resource
        .header("content-encoding")
        .unwrap_or_default()
        .trim()
    {
        "gzip" => read_limited(flate2::read::GzDecoder::new(resource.bytes.as_slice()))?,
        "deflate" => read_limited(flate2::read::ZlibDecoder::new(resource.bytes.as_slice()))?,
        "br" => read_limited(brotli::Decompressor::new(resource.bytes.as_slice(), 4096))?,
        "" | "identity" => resource.bytes.clone(),
        value => return Err(format!("不支持的压缩编码：{value}")),
    };
    let charset = resource.header("content-type").and_then(|header| {
        header
            .split(';')
            .find_map(|part| part.trim().strip_prefix("charset="))
    });
    parsing::decode(&bytes, charset)
}
fn read_limited(reader: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("解压正文超过 32 MiB".into());
    }
    Ok(bytes)
}
