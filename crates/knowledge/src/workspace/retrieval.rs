use super::*;
use crate::{Block, HitKind, MirroredPage, PAGE_SCHEMA_VERSION, Rebuilt, SearchHit};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryHit {
    pub library_id: String,
    pub library_name: String,
    #[serde(flatten)]
    pub hit: SearchHit,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub hits: Vec<LibraryHit>,
    pub warnings: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct VectorEntry {
    key: String,
    vector: Vec<f32>,
}
#[derive(Serialize, Deserialize)]
struct VectorCache {
    schema: u32,
    model: String,
    profile_revision: String,
    snapshot: String,
    entries: Vec<VectorEntry>,
}

impl Workspace {
    pub fn knowledge_base(&self, library: &Library) -> Result<Arc<KnowledgeBase>> {
        let mut indexes = self.indexes.lock().map_err(|_| "索引状态不可用")?;
        if let Some((revision, kb)) = indexes.get(&library.id)
            && revision == &library.revision
        {
            return Ok(Arc::clone(kb));
        }
        let snapshot = self.snapshot(library)?;
        let rebuilt = rebuilt(&snapshot);
        let kb = Arc::new(KnowledgeBase::new(rebuilt));
        indexes.insert(
            library.id.clone(),
            (library.revision.clone(), Arc::clone(&kb)),
        );
        Ok(kb)
    }
    pub fn documents(&self, id: &str, mcp: bool) -> Result<serde_json::Value> {
        let library = self.library(id, mcp)?;
        let snapshot = self.snapshot(&library)?;
        Ok(serde_json::json!(snapshot.documents.iter().map(|document| serde_json::json!({
            "id":document.id,"source_id":document.source_id,"title":document.title,"url":document.url,
            "updated_at":document.updated_at,"library_id":library.id,"library_name":library.name,"breadcrumb":document.metadata.breadcrumb,"section":document.metadata.section,
        })).collect::<Vec<_>>()))
    }
    pub fn document(&self, id: &str, document_id: &str, mcp: bool) -> Result<Document> {
        let library = self.library(id, mcp)?;
        self.snapshot(&library)?
            .documents
            .into_iter()
            .find(|document| document.id == document_id)
            .ok_or_else(|| "文档不存在，请刷新检索结果".into())
    }
    pub fn document_page(
        &self,
        id: &str,
        document_id: &str,
        offset: usize,
        anchor: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut document = self.document(id, document_id, false)?;
        let outline = document
            .elements
            .iter()
            .enumerate()
            .filter_map(|(index, element)| match element {
                crate::PageElement::Heading {
                    anchor,
                    text,
                    level,
                } => Some(
                    serde_json::json!({"anchor":anchor,"text":text,"level":level,"index":index}),
                ),
                _ => None,
            })
            .collect::<Vec<_>>();
        let total = document.elements.len();
        let start = if offset == 0 {
            anchor.and_then(|anchor|document.elements.iter().position(|element|matches!(element,crate::PageElement::Heading{anchor:value,..} if value==anchor))).unwrap_or(0)
        } else {
            offset
        };
        if start > total {
            return Err("阅读位置已失效，请刷新文档".into());
        }
        let mut size = 0;
        let mut end = start;
        while end < total && end - start < 200 {
            let bytes = serde_json::to_vec(&document.elements[end])
                .map_err(|error| error.to_string())?
                .len();
            if end > start && size + bytes > 256 * 1024 {
                break;
            }
            size += bytes;
            end += 1;
        }
        document.elements = document.elements.drain(start..end).collect();
        let mut value = serde_json::to_value(document).map_err(|error| error.to_string())?;
        value["outline"] = serde_json::json!(outline);
        value["next_offset"] = serde_json::json!(if end < total { Some(end) } else { None });
        value["element_offset"] = serde_json::json!(start);
        value["total_elements"] = serde_json::json!(total);
        Ok(value)
    }
    pub(crate) fn build_vectors(
        &self,
        library: &Library,
        transport: &dyn models::Transport,
    ) -> Result<()> {
        let id = library
            .model
            .as_ref()
            .ok_or("请先为这个库启用本地语义模型")?;
        let model = self.resolve_model(id)?;
        let profile = &model.profile;
        self.progress("正在连接本机嵌入服务", 0, 0);

        let snapshot = self.snapshot(library)?;
        let blocks = rebuilt(&snapshot).blocks;
        let path = self.directory(&library.id)?.join("vectors.json");
        let old: Option<VectorCache> = read_json(&path).ok();
        let previous = old
            .filter(|cache| {
                cache.schema == SCHEMA
                    && cache.model == *id
                    && cache.profile_revision == model.fingerprint()
            })
            .map(|cache| {
                cache
                    .entries
                    .into_iter()
                    .map(|entry| (entry.key, entry.vector))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        let mut entries = Vec::new();
        for (batch_index, batch) in blocks.chunks(16).enumerate() {
            self.check_cancel()?;
            let offset = batch_index * 16;
            self.progress(
                &format!("语义索引 · {}", batch[0].doc_title),
                offset,
                blocks.len(),
            );
            let missing = batch
                .iter()
                .filter(|block| {
                    !previous
                        .get(&vector_key(block))
                        .is_some_and(|vector| valid_vector(vector, profile.dimensions))
                })
                .map(embedding_text)
                .collect::<Vec<_>>();
            let vectors = self
                .pool
                .embed(&model, &missing, false, transport, &self.cancelled)?;
            let mut generated = vectors.into_iter();
            for block in batch {
                let key = vector_key(block);
                let vector = match previous
                    .get(&key)
                    .filter(|vector| valid_vector(vector, profile.dimensions))
                {
                    Some(vector) => vector.clone(),
                    None => generated.next().ok_or("模型未返回向量")?,
                };
                entries.push(VectorEntry { key, vector });
            }
        }
        self.check_cancel()?;
        let _guard = self.mutation.lock().map_err(|_| "知识库状态不可用")?;
        let mut registry = self.registry()?;
        let current = registry
            .libraries
            .iter_mut()
            .find(|current| current.id == library.id)
            .ok_or("知识库已删除")?;
        if current.revision != library.revision {
            return Err("知识库已改变，请重建语义索引".into());
        }
        let snapshot_id = library.snapshot.clone().ok_or("请先构建资料")?;
        atomic(
            &path,
            &VectorCache {
                schema: SCHEMA,
                model: id.clone(),
                profile_revision: model.fingerprint(),
                snapshot: snapshot_id,
                entries,
            },
        )?;
        current.vector_revision = Some(unique());
        current.revision = unique();
        self.save(&registry)?;
        self.progress("语义索引已构建", blocks.len(), blocks.len());
        Ok(())
    }
    pub fn search(
        &self,
        ids: &[String],
        query: &str,
        top_k: usize,
        mcp: bool,
    ) -> Result<SearchResult> {
        self.search_with(ids, query, top_k, mcp, &models::NativeTransport(&self.root))
    }
    pub fn search_with(
        &self,
        ids: &[String],
        query: &str,
        top_k: usize,
        mcp: bool,
        transport: &dyn models::Transport,
    ) -> Result<SearchResult> {
        if query.trim().is_empty() || query.len() > 8192 || top_k == 0 || top_k > 100 {
            return Err("请输入检索内容，结果数量应为 1–100".into());
        }
        let libraries = self.scope(ids, mcp)?;
        let mut all = Vec::new();
        let mut warnings = Vec::new();
        for library in libraries {
            let kb = self.knowledge_base(&library)?;
            let limit = (top_k * 3).clamp(20, 100);
            let sparse = crate::rag::plan_rag_search(&kb, &[query.into()], limit)
                .pop()
                .unwrap_or_default();
            let dense = if library.model.is_some() && library.vector_revision.is_some() {
                match self.dense(&library, query, limit, transport) {
                    Ok(hits) => hits,
                    Err(error) => {
                        warnings.push(format!(
                            "{}：语义检索不可用，已使用关键词检索。{error}",
                            library.name
                        ));
                        vec![]
                    }
                }
            } else {
                if library.model.is_some() {
                    warnings.push(format!(
                        "{}：语义索引尚未就绪，当前使用关键词检索",
                        library.name
                    ));
                }
                vec![]
            };
            let mut candidates: HashMap<String, (f64, SearchHit)> = HashMap::new();
            for route in [sparse, dense] {
                for (rank, hit) in route.into_iter().enumerate() {
                    let key = models::digest(
                        format!("{}:{}:{}", hit.path_id, hit.anchor, hit.text).as_bytes(),
                    );
                    let candidate = candidates.entry(key).or_insert((0.0, hit));
                    candidate.0 += 1.0 / (60.0 + rank as f64);
                }
            }
            for (key, (score, hit)) in candidates {
                all.push((
                    score,
                    format!("{}:{key}", library.id),
                    LibraryHit {
                        library_id: library.id.clone(),
                        library_name: library.name.clone(),
                        hit,
                    },
                ));
            }
        }
        all.sort_by(|left, right| {
            right
                .0
                .total_cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        // Revalidate after loading snapshots so revocation/deletion cannot return a stale library.
        let scope = self.scope(ids, mcp)?;
        all.retain(|(_, _, hit)| scope.iter().any(|library| library.id == hit.library_id));
        Ok(SearchResult {
            hits: all.into_iter().take(top_k).map(|(_, _, hit)| hit).collect(),
            warnings,
        })
    }
    fn dense(
        &self,
        library: &Library,
        query: &str,
        top_k: usize,
        transport: &dyn models::Transport,
    ) -> Result<Vec<SearchHit>> {
        let id = library.model.as_ref().ok_or("模型未启用")?;
        let model = self.resolve_model(id)?;
        let profile = &model.profile;
        let cache: VectorCache = read_json(&self.directory(&library.id)?.join("vectors.json"))?;
        if cache.schema != SCHEMA
            || cache.model != *id
            || cache.profile_revision != model.fingerprint()
            || Some(&cache.snapshot) != library.snapshot.as_ref()
        {
            return Err("语义索引已过期，请重新构建".into());
        }
        let vector = self
            .pool
            .query(&model, query, transport, &AtomicBool::new(false))?;
        let vectors = cache
            .entries
            .into_iter()
            .map(|entry| (entry.key, entry.vector))
            .collect::<HashMap<_, _>>();
        let kb = self.knowledge_base(library)?;
        let blocks = kb.blocks();
        let mut hits = Vec::new();
        for block in blocks {
            if let Some(candidate) = vectors
                .get(&vector_key(block))
                .filter(|candidate| valid_vector(candidate, profile.dimensions))
            {
                let score = vector
                    .iter()
                    .zip(candidate)
                    .map(|(left, right)| left * right)
                    .sum::<f32>();
                if score > 0.25 {
                    hits.push((score, hit(block.clone())));
                }
            }
        }
        hits.sort_by(|left, right| right.0.total_cmp(&left.0));
        Ok(hits.into_iter().take(top_k).map(|(_, hit)| hit).collect())
    }
}

fn valid_vector(vector: &[f32], dimensions: usize) -> bool {
    vector.len() == dimensions && vector.iter().all(|value| value.is_finite())
}
fn vector_key(block: &Block) -> String {
    models::digest(
        format!(
            "v1:{}:{}:{}",
            block.path_id,
            block.anchor,
            embedding_text(block)
        )
        .as_bytes(),
    )
}
fn embedding_text(block: &Block) -> String {
    format!(
        "{}\n{}\n{}",
        block.doc_title,
        block.heading_path.join(" / "),
        block.text
    )
}
fn hit(block: Block) -> SearchHit {
    SearchHit {
        path_id: block.path_id,
        doc_title: block.doc_title,
        anchor: block.anchor,
        breadcrumb: block.breadcrumb,
        text: block.text,
        side: block.side,
        node_graph: block.node_graph,
        updated_at: block.updated_at,
        url: block.url,
        kind: HitKind::Vector,
        table: block.table,
    }
}
fn rebuilt(snapshot: &Snapshot) -> Rebuilt {
    let mut built = Rebuilt {
        docs: vec![],
        blocks: vec![],
        nodes: vec![],
        chars: 0,
    };
    for document in &snapshot.documents {
        let page = MirroredPage {
            schema_version: PAGE_SCHEMA_VERSION,
            path_id: document.id.clone(),
            title: document.title.clone(),
            url: document.url.clone(),
            updated_at: document.metadata.updated_at.clone(),
            mirrored_at: document.updated_at,
            elements: document.elements.clone(),
        };
        let (blocks, nodes, _) = crate::chunk::build_page(&document.metadata, &page);
        for block in blocks {
            let chars = block
                .text
                .char_indices()
                .map(|(offset, _)| offset)
                .chain([block.text.len()])
                .collect::<Vec<_>>();
            let mut start = 0;
            while start + 1 < chars.len() {
                let end = (start + 384).min(chars.len() - 1);
                let mut part = block.clone();
                part.text = block.text[chars[start]..chars[end]].into();
                part.chars = end - start;
                built.chars += part.chars;
                built.blocks.push(part);
                if end == chars.len() - 1 {
                    break;
                }
                start = end.saturating_sub(32);
            }
        }
        built.nodes.extend(nodes);
        built.docs.push(document.metadata.clone());
    }
    built
}
