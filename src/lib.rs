//! 千星奇域知识库的 GUI 管理入口（插件后端）。
//!
//! 职责：知识模型、索引与基础召回由本插件的内部 crate 提供；本插件负责
//! 千星奇域场景识别、多路 RAG 检索规划与桌面入口。MCP 入口也调用这里的规划器，
//! 因而 GUI / MCP 对复杂问题使用同一套场景策略。本插件不接触任何凭据。
//!
//! 与 MCP 入口的分工：那边面向外部 AI 客户端（工具面 + 局限声明），这边面向人
//! （看缓存与索引状态、抓取 / 更新、检索与浏览、生成 MCP 配置片段）。
//! MCP 仍自行处理协议与只读进程生命周期，桌面壳仍处理 IPC 与系统集成。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tracing::{info, warn};
use wonderland_plugin_sdk::PluginFailure;
use wonderland_knowledge::cache::{Bundle, DEFAULT_MAX_AGE_DAYS, KnowledgeCache};
use wonderland_knowledge::mirror::{MirrorOptions, MirrorStore, mirror_all, mirror_one};
use wonderland_knowledge::render::DEFAULT_WAIT_MS;
use wonderland_knowledge::{
    DEFAULT_LANG, Doc, DocListGroup, DocText, HitKind, KnowledgeBase, KnowledgeStats, NO_MIRROR,
    NodeFilters, NodeLookup, SCHEMA_VERSION, SearchHit, Side, build_from_json,
};

pub mod model;
pub use model::*;
mod embedding;
mod rag;
pub use rag::plan_rag_search;
// 页面元素与镜像记录定义在共享知识库库（GUI 与 MCP 共用一份），这里只把它们带进 IPC 面。
pub use wonderland_knowledge::mirror::{MirrorProgress, MirrorRun, MirroredPage};
pub use wonderland_knowledge::page::{ListKind, PageCell, PageElement, PageRun};

#[cfg(test)]
mod tests;

/// MCP 二进制的文件名，与 `mcp/Cargo.toml` 的 `[[bin]] name` 一致。
#[cfg(windows)]
const MCP_BINARY: &str = "mcp-knowledge.exe";
#[cfg(not(windows))]
const MCP_BINARY: &str = "mcp-knowledge";

/// 找不到二进制时，配置片段里的占位路径（Windows 客户端 GUI 不继承 PATH，只能填绝对路径）。
const MCP_BINARY_PLACEHOLDER: &str = r"C:\path\to\mcp-knowledge.exe";

/// 没有 catalog 缓存时的可读说明：界面据此给出「获取目录」入口。
///
/// 正文与标题锚点都来自页面镜像（[`wonderland_knowledge::mirror`]），因此有 catalog
/// 但一页都没镜像时另有一句可读说明（[`NO_MIRROR`]）。
const NO_CACHE: &str = "本地还没有知识库目录，请先获取目录";

/// `search` 的 `top_k` 上限，与 MCP 工具声明一致（工具面守住同一套取值）。
const MAX_TOP_K: u32 = 20;

/// 一期知识库只有「综合指南」，语种固定 `zh-cn`（站点只提供这一份目录与镜像）。
pub struct KnowledgeLibrary {
    cache: KnowledgeCache,
    lang: String,
    /// 已建好的索引。重建要遍历全部正文，所以留在内存里；缓存刷新后置空。
    ///
    /// 单用户桌面、无并发检索需求，用 `Mutex` 而不是更复杂的缓存：重建只在首次查询
    /// 或刷新后发生一次。
    kb: Mutex<Option<Arc<KnowledgeBase>>>,
    embedding_error: Mutex<Option<String>>,
    /// 镜像作业状态：同时只允许一个，取消靠标志位（渲染是阻塞的子进程，不强杀）。
    mirror: Mutex<MirrorJob>,
}

/// 镜像作业的运行时状态。
#[derive(Default)]
struct MirrorJob {
    running: bool,
    cancel: Arc<AtomicBool>,
    last: Option<MirrorProgress>,
}

impl KnowledgeLibrary {
    /// Construct the plugin library over the shared GUI/MCP knowledge cache directory.
    pub fn new(cache_dir: PathBuf) -> Self {
        let lang = DEFAULT_LANG.to_owned();
        let cache = KnowledgeCache::new(cache_dir);
        info!(dir = %cache.dir().display(), "知识库缓存目录");
        Self {
            cache,
            lang,
            kb: Mutex::new(None),
            embedding_error: Mutex::new(None),
            mirror: Mutex::new(MirrorJob::default()),
        }
    }

    /// 缓存目录（界面展示；「打开缓存目录」也用它）。
    pub fn cache_dir(&self) -> PathBuf {
        self.cache.dir().to_path_buf()
    }

    /// 列出当前预设与磁盘上发现的模型目录，并区分设置和实际加载状态。
    pub fn embedding_status(&self) -> Result<KnowledgeEmbeddingStatus, PluginFailure> {
        let active = self
            .index()?
            .as_ref()
            .is_some_and(|base| base.has_local_embeddings());
        let error = self
            .embedding_error
            .lock()
            .map_err(|_| PluginFailure::NotInitialized)?
            .clone();
        Ok(KnowledgeEmbeddingStatus {
            active,
            error,
            models: embedding::model_inventory(self.cache.dir())?,
        })
    }

    /// 仅由模型管理窗口的显式操作调用；启动、检索和状态读取不会调用此方法。
    pub fn activate_embeddings(&self) -> Result<KnowledgeEmbeddingStatus, PluginFailure> {
        match self.build_base(true) {
            Ok(kb) => {
                *self.index()? = Some(kb);
                *self
                    .embedding_error
                    .lock()
                    .map_err(|_| PluginFailure::NotInitialized)? = None;
            }
            Err(error) => {
                *self
                    .embedding_error
                    .lock()
                    .map_err(|_| PluginFailure::NotInitialized)? = Some(error.to_string());
                return Err(error);
            }
        }
        self.embedding_status()
    }

    /// 明确停用并释放当前进程持有的嵌入模型。
    pub fn disable_embeddings(&self) -> Result<KnowledgeEmbeddingStatus, PluginFailure> {
        *self.index()? = None;
        *self
            .embedding_error
            .lock()
            .map_err(|_| PluginFailure::NotInitialized)? = None;
        self.embedding_status()
    }

    /// 复制或替换模型文件后重新读取清单；不会加载模型或构建向量索引。
    pub fn recheck_embeddings(&self) -> Result<KnowledgeEmbeddingStatus, PluginFailure> {
        *self
            .embedding_error
            .lock()
            .map_err(|_| PluginFailure::NotInitialized)? = None;
        self.embedding_status()
    }

    /// 缓存与索引状态。
    ///
    /// 没有可用缓存时**不报错**：这是正常的初始状态，界面靠 `ready = false` + `error`
    /// 给出「获取目录」入口。
    pub fn status(&self) -> KnowledgeStatus {
        let binary = mcp_binary();
        let mut status = KnowledgeStatus {
            ready: false,
            cache_dir: self.cache.dir().display().to_string(),
            cached_at: None,
            age_days: None,
            stale: false,
            schema_version: SCHEMA_VERSION,
            docs: 0,
            leaf_docs: 0,
            chars: 0,
            blocks: 0,
            nodes: 0,
            nodes_server: 0,
            nodes_client: 0,
            table_blocks: 0,
            error: None,
            mcp_config: mcp_config(binary.as_deref()),
            mcp_binary: binary.map(|path| path.display().to_string()),
        };

        match self.cache.load() {
            Ok(Some(bundle)) => {
                status.cached_at = Some(bundle.fetched_at as i64);
                status.age_days = Some(bundle.age_days());
                status.stale = !bundle.is_fresh(DEFAULT_MAX_AGE_DAYS);
                // 目录一落盘就应立即可浏览；正文统计会在下面有镜像时由完整索引覆盖。
                match build_from_json(&bundle.catalog, &[]) {
                    Ok(catalog) => apply_stats(&mut status, &catalog.stats()),
                    Err(error) => status.error = Some(error.to_string()),
                }
            }
            Ok(None) => status.error = Some(NO_CACHE.to_owned()),
            Err(error) => status.error = Some(error.to_string()),
        }

        // 缓存在但建不起索引（结构损坏等）同样按"不可用"处理，原因照原样给界面。
        if status.error.is_none() {
            match self.base() {
                Ok(kb) => {
                    apply_stats(&mut status, &kb.stats());
                    status.ready = true;
                }
                Err(error) => status.error = Some(error.to_string()),
            }
        }
        status
    }

    /// 抓取一次 catalog 并落缓存（界面上的「获取目录」/「更新目录」）。
    ///
    /// 抓的只是 catalog（目录树）；正文由页面镜像提供，因此更新目录不会动已镜像的页面。
    /// 与 MCP 侧的差别：这是用户的显式动作，所以**失败就报错**，不做"悄悄回退旧缓存"；
    /// 写盘失败也报错——否则界面会显示已更新而磁盘上并没有。
    pub fn refresh(&self, catalog: String) -> Result<KnowledgeRefreshReport, PluginFailure> {
        // Validate before replacing the cache, so an invalid Core response cannot overwrite
        // the last known-good catalog.
        let documents = build_from_json(&catalog, &[])?.stats().docs;
        let bundle = Bundle::new(&self.lang, catalog);
        // catalog 原文的字节数：这是"这次抓回来多少"的直观口径（缓存文件更大，含包装字段）。
        let bytes = bundle.catalog.len();
        // 目录篇数按 catalog 算（与有没有镜像无关，界面据此判断抓到了什么）。
        self.cache.store(&bundle)?;
        // 丢掉的只是内存索引，下次查询按新缓存重建；不动 MCP 进程正在读的文件。
        *self.index()? = None;
        let status = self.status();
        info!(documents, bytes, "知识库目录已更新");
        Ok(KnowledgeRefreshReport {
            documents,
            bytes,
            status,
        })
    }

    /// 普通检索（批量）：不做场景规划，直接委托给共享知识库库。
    pub fn search(
        &self,
        queries: Vec<String>,
        top_k: u32,
    ) -> Result<Vec<KnowledgeSearchGroup>, PluginFailure> {
        if !(1..=MAX_TOP_K).contains(&top_k) {
            return Err(PluginFailure::Other(format!(
                "top_k 必须在 1–{MAX_TOP_K} 之间，收到 {top_k}"
            )));
        }
        let queries = non_empty(queries, "queries")?;
        let kb = self.base()?;
        let groups = queries
            .iter()
            .zip(kb.search(&queries, top_k as usize))
            .map(|(query, hits)| KnowledgeSearchGroup {
                query: query.clone(),
                hits: hits.iter().map(hit).collect(),
            })
            .collect();
        Ok(groups)
    }

    /// 由知识库插件规划场景化 RAG；普通问题仍直接委托给内核检索。
    pub fn search_rag(
        &self,
        queries: Vec<String>,
        top_k: u32,
    ) -> Result<Vec<KnowledgeSearchGroup>, PluginFailure> {
        if !(1..=MAX_TOP_K).contains(&top_k) {
            return Err(PluginFailure::Other(format!(
                "top_k 必须在 1–{MAX_TOP_K} 之间，收到 {top_k}"
            )));
        }
        let queries = non_empty(queries, "queries")?;
        let kb = self.base()?;
        Ok(queries
            .iter()
            .zip(plan_rag_search(&kb, &queries, top_k as usize))
            .map(|(query, hits)| KnowledgeSearchGroup {
                query: query.clone(),
                hits: hits.iter().map(hit).collect(),
            })
            .collect())
    }

    /// 按节点名查说明（批量；同名节点整组返回）。
    pub fn get_node(&self, names: Vec<String>) -> Result<Vec<KnowledgeNodeGroup>, PluginFailure> {
        let names = non_empty(names, "names")?;
        Ok(self
            .base()?
            .get_node(&names)
            .iter()
            .map(node_group)
            .collect())
    }

    /// 按服务端/客户端和节点图类别筛选同名节点候选。
    pub fn get_node_scoped(
        &self,
        names: Vec<String>,
        side: Option<String>,
        node_graph: Option<String>,
    ) -> Result<Vec<KnowledgeNodeGroup>, PluginFailure> {
        let names = non_empty(names, "names")?;
        let filters = NodeFilters {
            side: parse_side(side)?,
            node_graph: clean_optional(node_graph),
        };
        Ok(self
            .base()?
            .get_node_scoped(&names, &filters)
            .iter()
            .map(node_group)
            .collect())
    }

    /// 列目录标题（批量；关键词留空即列全部——界面目录树走这里）。
    ///
    /// 目录只依赖 catalog，不应被“还没有正文镜像”阻塞；这样目录阶段完成后前端可以立即浏览
    /// 当前目录中的全部目标，并从任意条目触发单篇镜像。
    pub fn list_docs(&self, keywords: Vec<String>) -> Result<Vec<KnowledgeDocGroup>, PluginFailure> {
        let keywords: Vec<String> = keywords
            .into_iter()
            .map(|keyword| keyword.trim().to_owned())
            .filter(|keyword| !keyword.is_empty())
            .collect();
        let bundle = self
            .cache
            .load()?
            .ok_or_else(|| PluginFailure::LocalData(NO_CACHE.to_owned()))?;
        Ok(build_from_json(&bundle.catalog, &[])?
            .list_docs(&keywords)
            .iter()
            .map(doc_group)
            .collect())
    }

    /// 按 `path_id` 或标题读整篇（可选从某个锚点起）。
    pub fn read_doc(
        &self,
        key: String,
        from_anchor: Option<String>,
    ) -> Result<KnowledgeDocText, PluginFailure> {
        let key = key.trim().to_owned();
        if key.is_empty() {
            return Err(PluginFailure::Other("请给出文档标题或 path_id".to_owned()));
        }
        let from_anchor = from_anchor
            .map(|anchor| anchor.trim().to_owned())
            .filter(|anchor| !anchor.is_empty());
        self.base()?
            .read_doc(&key, from_anchor.as_deref())
            .map(|doc| doc_text(&doc))
            .ok_or_else(|| {
                // 与 MCP 侧同一句话：查不到 ≠ 知识库里没有资料，这里只说没匹配到。
                PluginFailure::Other(format!("知识库中没有标题或 path_id 匹配「{key}」的文档"))
            })
    }

    /// 文档的官方原文链接（调研 §6.4 的出处要求）。
    ///
    /// `path_id` 必须命中目录：前端只能传标识、不能传任意地址，宿主据此打开浏览器。
    pub fn source_url(&self, path_id: &str) -> Result<String, PluginFailure> {
        self.catalog_docs()?
            .iter()
            .find(|doc| doc.path_id == path_id)
            .map(|doc| doc.url.clone())
            .ok_or_else(|| PluginFailure::Other(format!("知识库中没有 path_id 为 {path_id} 的文档")))
    }

    /// 镜像产物存放处（与 catalog 缓存同一目录下的 `pages/` 与 `mirror.json`）。
    fn store(&self) -> MirrorStore {
        MirrorStore::new(self.cache.dir().to_path_buf())
    }

    /// 镜像状态：已镜像多少、还要镜像多少、有没有在跑。
    ///
    /// 这里只按 catalog 取文档树（**不要求已有镜像**）：否则"一页都没镜像"时连面板都打不开。
    pub fn mirror_status(&self) -> Result<KnowledgeMirrorStatus, PluginFailure> {
        let index = self.store().load_index();
        let docs = self.catalog_docs()?;
        // 官方 `updated_at` 与索引里记的不一致 → 这篇要重镜像。
        let outdated = index
            .pages
            .iter()
            .filter(|(path_id, meta)| {
                docs.iter()
                    .find(|doc| &doc.path_id == *path_id)
                    .is_some_and(|doc| doc.updated_at != meta.updated_at)
            })
            .count();
        let job = self
            .mirror
            .lock()
            .map_err(|_| PluginFailure::NotInitialized)?;
        Ok(KnowledgeMirrorStatus {
            mirrored: index.pages(),
            // catalog 的目录节点也有正文；总数始终以当前目录为准。
            total: docs.len(),
            outdated,
            elements: index.elements(),
            images: index.images(),
            running: job.running,
            last: job.last.clone(),
        })
    }

    /// 批量镜像页面元素（界面上的「镜像知识库」）；`limit` 只镜像前几篇。
    ///
    /// 页面以固定小并发渲染，每完成一篇回调一次；`updated_at` 未变的篇目直接跳过，
    /// 因此中途取消后再点一次就能接着做。
    pub async fn mirror(
        &self,
        limit: Option<usize>,
        mut progress: impl FnMut(MirrorProgress),
    ) -> Result<MirrorRun, PluginFailure> {
        // 先确认目录可用，再占住作业位：失败不该把状态卡在"运行中"。
        // 用 catalog 的文档树（含尚未镜像的篇目），不能走 `base()`——那要求已经有镜像。
        let docs = self.catalog_docs()?;
        let cancel = {
            let mut job = self
                .mirror
                .lock()
                .map_err(|_| PluginFailure::NotInitialized)?;
            if job.running {
                return Err(PluginFailure::Other("镜像正在进行，请等待完成".to_owned()));
            }
            job.running = true;
            job.cancel.store(false, Ordering::SeqCst);
            job.last = None;
            Arc::clone(&job.cancel)
        };

        let store = self.store();
        let outcome = mirror_all(
            &store,
            &docs,
            MirrorOptions {
                limit,
                ..Default::default()
            },
            |report| {
                if let Ok(mut job) = self.mirror.lock() {
                    job.last = Some(report.clone());
                }
                progress(report);
            },
            || cancel.load(Ordering::SeqCst),
        )
        .await;

        if let Ok(mut job) = self.mirror.lock() {
            job.running = false;
        }
        let (run, _) = outcome?;
        info!(
            mirrored = run.mirrored,
            skipped = run.skipped,
            failed = run.failed,
            "镜像作业结束"
        );
        Ok(run)
    }

    /// 请正在跑的镜像作业停下；下一次翻篇前生效，已镜像的篇目留在盘上。
    pub fn cancel_mirror(&self) {
        if let Ok(job) = self.mirror.lock() {
            job.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// 镜像单篇（点开某篇但还没镜像时用）。
    ///
    /// 同样只依赖 catalog：这一篇**正是还没镜像**的那一篇。
    pub async fn mirror_page(&self, path_id: &str) -> Result<MirroredPage, PluginFailure> {
        if self
            .mirror
            .lock()
            .map_err(|_| PluginFailure::NotInitialized)?
            .running
        {
            return Err(PluginFailure::Other(
                "全库镜像正在进行，请等待完成或先停止任务".to_owned(),
            ));
        }
        let docs = self.catalog_docs()?;
        let doc = docs
            .iter()
            .find(|doc| doc.path_id == path_id)
            .ok_or_else(|| {
                PluginFailure::Other(format!("知识库中没有 path_id 为 {path_id} 的文档"))
            })?;
        let page = mirror_one(&self.store(), doc, DEFAULT_WAIT_MS).await?;
        info!(path_id, elements = page.elements.len(), "页面已镜像");
        Ok(page)
    }

    /// 读一篇已镜像的页面；没镜像过为 `None`（界面据此提示先镜像）。
    pub fn page(&self, path_id: &str) -> Result<Option<MirroredPage>, PluginFailure> {
        self.store().load_page(path_id)
    }

    /// 取索引：catalog + 已镜像的页面。
    ///
    /// 两种"还没准备好"都给出可读原因：没抓过目录 → 引导抓取；有目录但一页没镜像 → 引导镜像。
    fn base(&self) -> Result<Arc<KnowledgeBase>, PluginFailure> {
        let mut memo = self.index()?;
        if let Some(kb) = memo.as_ref() {
            return Ok(Arc::clone(kb));
        }
        let kb = self.build_base(false)?;
        *memo = Some(Arc::clone(&kb));
        Ok(kb)
    }

    fn build_base(&self, with_embeddings: bool) -> Result<Arc<KnowledgeBase>, PluginFailure> {
        let bundle = self
            .cache
            .load()?
            .ok_or_else(|| PluginFailure::LocalData(NO_CACHE.to_owned()))?;
        let pages = self.store().load_pages()?;
        if pages.is_empty() {
            return Err(PluginFailure::LocalData(NO_MIRROR.to_owned()));
        }
        let mut built = build_from_json(&bundle.catalog, &pages)?;
        if with_embeddings {
            built.enable_local_embeddings(self.cache.dir(), true)?;
        }
        let kb = Arc::new(built);
        info!(
            docs = kb.stats().docs,
            blocks = kb.stats().blocks,
            "知识库索引已建立"
        );
        Ok(kb)
    }

    /// 只由 catalog 得到的文档树（镜像作业与原文链接用）。
    ///
    /// 镜像作业要列出**全部**待镜像篇目，因此不能走 [`Self::base`]——那要求已经有镜像。
    fn catalog_docs(&self) -> Result<Vec<Doc>, PluginFailure> {
        let bundle = self
            .cache
            .load()?
            .ok_or_else(|| PluginFailure::LocalData(NO_CACHE.to_owned()))?;
        Ok(build_from_json(&bundle.catalog, &[])?.docs().to_vec())
    }

    /// 索引缓存句柄。锁中毒只可能来自重建时的 panic，按内核既有做法回 `NotInitialized`。
    fn index(&self) -> Result<std::sync::MutexGuard<'_, Option<Arc<KnowledgeBase>>>, PluginFailure> {
        self.kb.lock().map_err(|_| PluginFailure::NotInitialized)
    }
}

/// 非空的字符串列表；空列表或含空白项都算"没有可用输入"。
///
/// 与 MCP 工具面对同样的输入给同样的拒绝理由，界面与外部客户端的行为才一致。
fn non_empty(values: Vec<String>, field: &str) -> Result<Vec<String>, PluginFailure> {
    let values: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .collect();
    if values.is_empty() || values.iter().any(String::is_empty) {
        return Err(PluginFailure::Other(format!("{field} 至少要有一个非空项")));
    }
    Ok(values)
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_side(side: Option<String>) -> Result<Option<Side>, PluginFailure> {
    match clean_optional(side).as_deref() {
        None => Ok(None),
        Some("server") => Ok(Some(Side::Server)),
        Some("client") => Ok(Some(Side::Client)),
        Some(value) => Err(PluginFailure::Other(format!(
            "side 仅支持 server 或 client，收到 {value}"
        ))),
    }
}

fn apply_stats(status: &mut KnowledgeStatus, stats: &KnowledgeStats) {
    status.docs = stats.docs;
    status.leaf_docs = stats.leaf_docs;
    status.chars = stats.chars;
    status.blocks = stats.blocks;
    status.nodes = stats.nodes;
    status.nodes_server = stats.nodes_server;
    status.nodes_client = stats.nodes_client;
    status.table_blocks = stats.tables;
}

/// 归属端转成与 MCP 一致的字符串（内核枚举 `snake_case`/`lowercase` 的取值）。
fn side(side: Option<Side>) -> Option<String> {
    side.map(|side| {
        match side {
            Side::Server => "server",
            Side::Client => "client",
            Side::Both => "both",
        }
        .to_owned()
    })
}

/// 命中来源转成与 MCP 一致的字符串。
fn kind(kind: HitKind) -> String {
    match kind {
        HitKind::CandidateNode => "candidate_node",
        HitKind::CandidateDoc => "candidate_doc",
        HitKind::Sparse => "sparse",
        HitKind::Vector => "vector",
    }
    .to_owned()
}

fn hit(hit: &SearchHit) -> KnowledgeHit {
    KnowledgeHit {
        path_id: hit.path_id.clone(),
        doc_title: hit.doc_title.clone(),
        breadcrumb: hit.breadcrumb.clone(),
        anchor: hit.anchor.clone(),
        side: side(hit.side),
        node_graph: hit.node_graph.clone(),
        updated_at: hit.updated_at.clone(),
        url: hit.url.clone(),
        text: hit.text.clone(),
        kind: kind(hit.kind),
        table: hit.table.clone(),
    }
}

fn node_group(lookup: &NodeLookup) -> KnowledgeNodeGroup {
    KnowledgeNodeGroup {
        query: lookup.query.clone(),
        matches: lookup
            .matches
            .iter()
            .map(|candidate| KnowledgeNodeCandidate {
                name: candidate.name.clone(),
                path_id: candidate.path_id.clone(),
                doc_title: candidate.doc_title.clone(),
                breadcrumb: candidate.breadcrumb.clone(),
                anchor: candidate.anchor.clone(),
                side: side(candidate.side),
                node_graph: candidate.node_graph.clone(),
                params: candidate.params.clone(),
                params_need_render: candidate.params_need_render,
                url: candidate.url.clone(),
            })
            .collect(),
    }
}

fn doc_group(group: &DocListGroup) -> KnowledgeDocGroup {
    KnowledgeDocGroup {
        keyword: group.keyword.clone(),
        total: group.total,
        docs: group
            .docs
            .iter()
            .map(|doc| KnowledgeDocBrief {
                path_id: doc.path_id.clone(),
                title: doc.title.clone(),
                breadcrumb: doc.breadcrumb.clone(),
                section: doc.section.clone(),
                is_leaf: doc.is_leaf,
                updated_at: doc.updated_at.clone(),
            })
            .collect(),
    }
}

fn doc_text(doc: &DocText) -> KnowledgeDocText {
    KnowledgeDocText {
        path_id: doc.path_id.clone(),
        title: doc.title.clone(),
        url: doc.url.clone(),
        updated_at: doc.updated_at.clone(),
        side: side(doc.side),
        node_graph: doc.node_graph.clone(),
        has_tables: doc.has_tables,
        blocks: doc.blocks.iter().map(hit).collect(),
    }
}

/// 随应用一起安装的 MCP 二进制：**应用 exe 的同级目录**。
///
/// 开发构建下 `wonderland-desktop.exe` 与 `mcp-knowledge.exe` 同在 `target/debug`，天然命中；
/// 安装版要由打包配置把二进制放进同级目录（ADR-010「分发与发现」），这里只做检测、
/// 不猜路径、不去 PATH 里找（Windows 客户端 GUI 不继承 PATH）。
fn mcp_binary() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join(MCP_BINARY);
    path.is_file().then_some(path)
}

/// 现成的 MCP 客户端配置片段（ADR-010：**只生成、不自动改写第三方配置**）。
///
/// 找不到二进制时给模板并在根上附一条 `_note`，用户复制前能看见该改哪里。
fn mcp_config(binary: Option<&Path>) -> String {
    let found = binary.is_some();
    let command = binary
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| MCP_BINARY_PLACEHOLDER.to_owned());
    let mut root = json!({
        "mcpServers": {
            "wonderland-knowledge": { "command": command, "args": [] }
        }
    });
    if !found {
        root["_note"] = json!(format!(
            "未在应用目录找到 {MCP_BINARY}：请先把 command 换成它的绝对路径，再粘贴到客户端配置里。"
        ));
    }
    match serde_json::to_string_pretty(&root) {
        Ok(text) => text,
        Err(error) => {
            warn!(reason = %error, "MCP 配置片段序列化失败");
            String::new()
        }
    }
}
