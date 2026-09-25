//! 镜像落盘与批量作业：把每篇文档的页面元素存成一份 JSON，并维护一份轻量索引。
//!
//! 目录布局（与 catalog 缓存同一份，见 [`crate::cache`]）：
//!
//! ```text
//! <app_data>/plugins/knowledge/<lang>/v1/
//!   bundle.json          catalog.json 原文（目录树：分区 / 标题 / path_id / updated_at）
//!   pages/<path_id>.json 一篇的页面元素（**唯一的内容源**：检索与前端还原排版都用它）
//!   mirror.json          镜像索引：每篇的镜像时刻、文档 updated_at 与规模
//! ```
//!
//! 为什么索引单独存：判断"要不要重新镜像"只需要 `updated_at` 与规模，不该为了看一眼状态
//! 去读大量页面 JSON。
//!
//! 作业约定：
//! - **有界并行**：目录先完整落盘，再以小并发渲染页面，避免所有页面串行等待，
//!   同时限制对官方站点与本机 Edge 的压力；
//! - **可中断、可续做**：`updated_at` 未变的篇目直接跳过；每篇写完就更新索引；
//! - **单篇失败不阻塞**：连续失败到阈值才整体报错（问题多半是环境性的，例如 Edge 不可用）；
//! - **正文为空的篇目算失败**：真正的文档页一定有 `div.doc-view`，抽不出来说明官方版式变了，
//!   存下去只会让界面显示空页。

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::model::Doc;
use crate::page::{PAGE_SCHEMA_VERSION, PageElement, elements_from_dom};
use crate::render::{DEFAULT_WAIT_MS, detail_url, dump_dom};
use wonderland_plugin_sdk::PluginFailure;
use tracing::{info, warn};

/// 连续失败多少篇就认定是环境问题并中止（单篇失败照旧跳过）。
const FAILURE_LIMIT: usize = 3;

/// 页面渲染默认并发数。Edge 页面本身较重，固定小并发比按 CPU 数无上限扩张更稳妥。
pub const DEFAULT_MIRROR_CONCURRENCY: usize = 4;

/// 防止调用方误配出大量 Edge 进程。
const MAX_MIRROR_CONCURRENCY: usize = 8;

/// 镜像产物的结构版本；与 [`PAGE_SCHEMA_VERSION`] 一起参与校验。
pub const MIRROR_SCHEMA_VERSION: u32 = 1;

/// 一篇镜像后的页面。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct MirroredPage {
    pub schema_version: u32,
    pub path_id: String,
    pub title: String,
    pub url: String,
    /// 镜像时刻对应目录里的 `updated_at`：用它判断要不要重新镜像。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[cfg_attr(feature = "bindings", ts(type = "number"))]
    pub mirrored_at: u64,
    pub elements: Vec<PageElement>,
}

impl MirroredPage {
    pub fn tables(&self) -> usize {
        self.count(|element| matches!(element, PageElement::Table { .. }))
    }

    pub fn images(&self) -> usize {
        self.elements
            .iter()
            .map(|element| match element {
                PageElement::Image { .. } => 1,
                PageElement::Table { rows, .. } => {
                    rows.iter().flatten().map(|cell| cell.images.len()).sum()
                }
                _ => 0,
            })
            .sum()
    }

    fn count(&self, pred: impl Fn(&PageElement) -> bool) -> usize {
        self.elements.iter().filter(|element| pred(element)).count()
    }
}

/// 镜像索引里的一条记录（轻量，不含元素本体）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageMeta {
    pub mirrored_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub elements: usize,
    pub tables: usize,
    pub images: usize,
}

/// 镜像索引。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorIndex {
    pub schema_version: u32,
    /// `path_id → 记录`；用 `BTreeMap` 让落盘结果稳定、便于人看。
    pub pages: BTreeMap<String, PageMeta>,
}

impl Default for MirrorIndex {
    fn default() -> Self {
        Self {
            schema_version: MIRROR_SCHEMA_VERSION,
            pages: BTreeMap::new(),
        }
    }
}

impl MirrorIndex {
    pub fn is_current(&self, path_id: &str, updated_at: Option<&str>) -> bool {
        self.pages
            .get(path_id)
            .is_some_and(|meta| meta.updated_at.as_deref() == updated_at)
    }

    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    pub fn elements(&self) -> usize {
        self.pages.values().map(|meta| meta.elements).sum()
    }

    pub fn images(&self) -> usize {
        self.pages.values().map(|meta| meta.images).sum()
    }
}

/// 镜像产物的读写。
pub struct MirrorStore {
    dir: PathBuf,
}

impl MirrorStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn pages_dir(&self) -> PathBuf {
        self.dir.join("pages")
    }

    fn page_path(&self, path_id: &str) -> PathBuf {
        self.pages_dir()
            .join(format!("{}.json", safe_name(path_id)))
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("mirror.json")
    }

    /// 读索引；文件不存在或结构版本不符按"还没镜像"处理。
    pub fn load_index(&self) -> MirrorIndex {
        let path = self.index_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return MirrorIndex::default();
        };
        match serde_json::from_str::<MirrorIndex>(&raw) {
            Ok(index) if index.schema_version == MIRROR_SCHEMA_VERSION => index,
            Ok(index) => {
                warn!(
                    found = index.schema_version,
                    "镜像索引版本不匹配，按未镜像处理"
                );
                MirrorIndex::default()
            }
            Err(error) => {
                warn!(reason = %error, "镜像索引解析失败，按未镜像处理");
                MirrorIndex::default()
            }
        }
    }

    /// 原子写索引（临时文件 + rename），与 catalog 缓存同一做法。
    pub fn save_index(&self, index: &MirrorIndex) -> Result<(), PluginFailure> {
        std::fs::create_dir_all(&self.dir).map_err(storage)?;
        let bytes = serde_json::to_vec_pretty(index).map_err(storage)?;
        let temp = self
            .dir
            .join(format!("mirror.json.tmp-{}", std::process::id()));
        std::fs::write(&temp, bytes).map_err(storage)?;
        std::fs::rename(&temp, self.index_path()).map_err(storage)
    }

    /// 读回**全部**已镜像的页面（按 `mirror.json` 索引的键；缺文件的条目跳过）。
    ///
    /// 这是索引构建的输入：镜像为空即"还没有镜像"，由入口给出可读提示。
    pub fn load_pages(&self) -> Result<Vec<MirroredPage>, PluginFailure> {
        let index = self.load_index();
        let mut pages = Vec::with_capacity(index.pages.len());
        for path_id in index.pages.keys() {
            if let Some(page) = self.load_page(path_id)? {
                pages.push(page);
            }
        }
        Ok(pages)
    }

    /// 读一篇的页面元素；不存在为 `None`。
    pub fn load_page(&self, path_id: &str) -> Result<Option<MirroredPage>, PluginFailure> {
        let path = self.page_path(path_id);
        if !path.is_file() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path).map_err(storage)?;
        let page: MirroredPage = serde_json::from_str(&raw).map_err(|error| {
            PluginFailure::LocalData(format!("镜像的页面无法解析（{}）：{error}", path.display()))
        })?;
        if page.schema_version != PAGE_SCHEMA_VERSION {
            warn!(
                found = page.schema_version,
                "页面镜像版本不匹配，按未镜像处理"
            );
            return Ok(None);
        }
        Ok(Some(page))
    }

    /// 写一篇的页面元素（临时文件 + rename）。
    pub fn save_page(&self, page: &MirroredPage) -> Result<(), PluginFailure> {
        std::fs::create_dir_all(self.pages_dir()).map_err(storage)?;
        let path = self.page_path(&page.path_id);
        let temp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(page).map_err(storage)?;
        std::fs::write(&temp, bytes).map_err(storage)?;
        std::fs::rename(&temp, &path).map_err(storage)
    }
}

/// 一次作业的开关。
#[derive(Debug, Clone, Copy)]
pub struct MirrorOptions {
    /// 渲染等待（毫秒），透传给 [`dump_dom`]。
    pub wait_ms: u64,
    /// 最多镜像多少篇（`None` = 全部应镜像的篇目）；先跑一小批用。
    pub limit: Option<usize>,
    /// 同时渲染的页面数；实际值会夹在 1–8。
    pub concurrency: usize,
}

impl Default for MirrorOptions {
    fn default() -> Self {
        Self {
            wait_ms: DEFAULT_WAIT_MS,
            limit: None,
            concurrency: DEFAULT_MIRROR_CONCURRENCY,
        }
    }
}

/// 每篇镜像完成后的进度（含失败的那一篇）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct MirrorProgress {
    pub path_id: String,
    pub title: String,
    /// 已处理篇数（含跳过与失败）。
    pub done: usize,
    pub total: usize,
    /// 当前索引里已有多少篇；成功落盘后立即更新，供前端摘要同步递增。
    pub mirrored: usize,
    pub elements: usize,
    pub tables: usize,
    pub images: usize,
    /// 这一篇是否因"已是新的"被跳过（此时只有 `done` / `total` 有意义）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    /// 这一篇失败的原因；成功与跳过为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
}

/// 一次作业的结果。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct MirrorRun {
    pub mirrored: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// 渲染并抽取一篇（不落盘）。
pub async fn render_page(doc: &Doc, wait_ms: u64) -> Result<MirroredPage, PluginFailure> {
    let url = if doc.url.is_empty() {
        detail_url(&doc.path_id)
    } else {
        doc.url.clone()
    };
    // Edge 渲染是阻塞调用，放进阻塞线程池：宿主是多线程运行时，别把工作线程占住十余秒。
    let html = tokio::task::spawn_blocking(move || dump_dom(&url, wait_ms))
        .await
        .map_err(|error| PluginFailure::Other(format!("渲染任务失败：{error}")))??;

    let elements = elements_from_dom(&html);
    if elements.is_empty() {
        return Err(PluginFailure::Other(
            "正文没有提取到任何元素，官方详情页版式可能已变化".to_owned(),
        ));
    }
    Ok(MirroredPage {
        schema_version: PAGE_SCHEMA_VERSION,
        path_id: doc.path_id.clone(),
        title: doc.title.clone(),
        url: doc.url.clone(),
        updated_at: doc.updated_at.clone(),
        mirrored_at: now_secs(),
        elements,
    })
}

/// 镜像一篇并落盘。
pub async fn mirror_one(
    store: &MirrorStore,
    doc: &Doc,
    wait_ms: u64,
) -> Result<MirroredPage, PluginFailure> {
    let page = render_page(doc, wait_ms).await?;
    store.save_page(&page)?;
    let index = store.load_index();
    store.save_index(&with_page(index, &page))?;
    Ok(page)
}

/// 批量镜像：跳过 `updated_at` 未变的篇目，以有界并发渲染，其后由协调线程逐篇落盘并回调进度。
///
/// `is_cancelled` 每篇开始前问一次；取消后正常返回已完成的部分（不报错、也不丢已写的产物）。
pub async fn mirror_all<F, C>(
    store: &MirrorStore,
    docs: &[Doc],
    options: MirrorOptions,
    mut progress: F,
    is_cancelled: C,
) -> Result<(MirrorRun, MirrorIndex), PluginFailure>
where
    F: FnMut(MirrorProgress),
    C: Fn() -> bool,
{
    let mut index = store.load_index();
    // `is_leaf` 只描述目录层级，不代表目录节点没有正文。
    // 因此目录页也必须镜像，否则会静默漏掉约 31 篇正文。
    // 先形成稳定作业清单：目录是调度源，页面任务只拿克隆后的条目，不在并发任务里碰 catalog。
    // `limit` 沿用原语义——限制本次新渲染数量，已是最新的页面仍会在抵达限制前报告跳过。
    let mut new_pages = 0usize;
    let mut targets = VecDeque::new();
    if options.limit != Some(0) {
        for doc in docs {
            let current = index.is_current(&doc.path_id, doc.updated_at.as_deref());
            if !current {
                if options.limit.is_some_and(|limit| new_pages >= limit) {
                    break;
                }
                new_pages += 1;
            }
            targets.push_back((doc.clone(), current));
        }
    }
    let total = targets.len();
    let mut run = MirrorRun::default();
    let mut consecutive_failures = 0usize;
    let concurrency = options.concurrency.clamp(1, MAX_MIRROR_CONCURRENCY);
    let mut tasks = tokio::task::JoinSet::new();
    let mut done = 0usize;
    let mut stopped = false;
    let mut fatal_error = None;

    while !targets.is_empty() || !tasks.is_empty() {
        if is_cancelled() && !stopped {
            stopped = true;
            targets.clear();
            info!(
                done,
                in_flight = tasks.len(),
                "镜像已取消，等待在途页面收尾"
            );
        }

        while !stopped && tasks.len() < concurrency {
            let Some((doc, current)) = targets.pop_front() else {
                break;
            };
            if current {
                done += 1;
                run.skipped += 1;
                progress(MirrorProgress {
                    path_id: doc.path_id,
                    title: doc.title,
                    done,
                    total,
                    mirrored: index.pages(),
                    elements: 0,
                    tables: 0,
                    images: 0,
                    skipped: true,
                    failed: None,
                });
                continue;
            }
            let wait_ms = options.wait_ms;
            tasks.spawn(async move {
                let outcome = render_page(&doc, wait_ms).await;
                (doc, outcome)
            });
        }

        let Some(joined) = tasks.join_next().await else {
            break;
        };
        let (doc, outcome) =
            joined.map_err(|error| PluginFailure::Other(format!("页面镜像任务异常结束：{error}")))?;
        done += 1;
        let mut report = MirrorProgress {
            path_id: doc.path_id.clone(),
            title: doc.title.clone(),
            done,
            total,
            mirrored: index.pages(),
            elements: 0,
            tables: 0,
            images: 0,
            skipped: false,
            failed: None,
        };
        match outcome {
            Ok(page) => {
                // 先落页面、再更新索引：中途失败时宁可重镜像，也不要出现"索引说有、文件没有"。
                store.save_page(&page)?;
                index = with_page(index, &page);
                store.save_index(&index)?;
                report.mirrored = index.pages();
                report.elements = page.elements.len();
                report.tables = page.tables();
                report.images = page.images();
                run.mirrored += 1;
                consecutive_failures = 0;
                info!(
                    path_id = %page.path_id,
                    elements = page.elements.len(),
                    tables = report.tables,
                    "页面已镜像"
                );
            }
            Err(error) => {
                report.failed = Some(error.to_string());
                run.failed += 1;
                consecutive_failures += 1;
                warn!(path_id = %doc.path_id, code = error.code(), reason = %error, "镜像失败");
                if consecutive_failures >= FAILURE_LIMIT {
                    stopped = true;
                    targets.clear();
                    fatal_error = Some(PluginFailure::Other(format!(
                        "连续 {FAILURE_LIMIT} 篇镜像失败，已中止（最近一次：{error}）"
                    )));
                }
            }
        }
        progress(report);
    }
    if let Some(error) = fatal_error {
        return Err(error);
    }
    Ok((run, index))
}

fn with_page(mut index: MirrorIndex, page: &MirroredPage) -> MirrorIndex {
    index.pages.insert(
        page.path_id.clone(),
        PageMeta {
            mirrored_at: page.mirrored_at,
            updated_at: page.updated_at.clone(),
            elements: page.elements.len(),
            tables: page.tables(),
            images: page.images(),
        },
    );
    index
}

/// 文件名只允许 `path_id` 这种形式（官方给的是小写字母数字），其余一律挡掉。
fn safe_name(path_id: &str) -> String {
    path_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn storage(error: impl std::fmt::Display) -> PluginFailure {
    PluginFailure::LocalData(format!("镜像读写失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{ListKind, PageRun};

    fn store(tag: &str) -> MirrorStore {
        let dir = std::env::temp_dir().join(format!(
            "wonderland-knowledge-mirror-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        MirrorStore::new(dir)
    }

    fn page(path_id: &str, updated_at: Option<&str>) -> MirroredPage {
        MirroredPage {
            schema_version: PAGE_SCHEMA_VERSION,
            path_id: path_id.to_owned(),
            title: "示例文档".to_owned(),
            url: detail_url(path_id),
            updated_at: updated_at.map(str::to_owned),
            mirrored_at: now_secs(),
            elements: vec![
                PageElement::Heading {
                    level: 1,
                    text: "一、示例".to_owned(),
                    anchor: "a1".to_owned(),
                },
                PageElement::Paragraph {
                    runs: vec![PageRun {
                        text: "正文".to_owned(),
                        bold: true,
                        italic: false,
                        color: None,
                        link: None,
                    }],
                },
                PageElement::List {
                    kind: ListKind::Bullet,
                    indent: 1,
                    items: vec![vec![PageRun {
                        text: "一项".to_owned(),
                        bold: false,
                        italic: false,
                        color: None,
                        link: None,
                    }]],
                },
            ],
        }
    }

    fn doc(path_id: &str, updated_at: Option<&str>) -> Doc {
        Doc {
            path_id: path_id.to_owned(),
            title: format!("文档 {path_id}"),
            section: "测试".to_owned(),
            breadcrumb: vec!["测试".to_owned(), format!("文档 {path_id}")],
            is_leaf: true,
            updated_at: updated_at.map(str::to_owned),
            node_graph: None,
            side: None,
            url: detail_url(path_id),
        }
    }

    #[test]
    fn page_roundtrip_keeps_elements() {
        let store = store("roundtrip");
        let page = page("mh0000000001", Some("2026-08-01 10:00:00"));
        assert!(store.load_page("mh0000000001").unwrap().is_none());

        store.save_page(&page).unwrap();
        let loaded = store.load_page("mh0000000001").unwrap().expect("应能读回");
        assert_eq!(loaded, page);
        assert_eq!(loaded.elements.len(), 3);
    }

    #[test]
    fn index_marks_pages_current_only_when_updated_at_matches() {
        let store = store("index");
        let index = with_page(
            MirrorIndex::default(),
            &page("mh0000000002", Some("2026-08-01")),
        );
        store.save_index(&index).unwrap();

        let loaded = store.load_index();
        assert!(loaded.is_current("mh0000000002", Some("2026-08-01")));
        assert!(
            !loaded.is_current("mh0000000002", Some("2026-09-01")),
            "官方更新后要重镜像"
        );
        assert!(!loaded.is_current("mh0000000003", Some("2026-08-01")));
        assert_eq!(loaded.pages(), 1);
    }

    #[test]
    fn missing_or_corrupt_files_never_panic() {
        let store = store("broken");
        // 索引不存在 → 空索引
        assert_eq!(store.load_index().pages(), 0);
        // 页面不存在 → None
        assert!(store.load_page("mh0000000004").unwrap().is_none());

        // 页面内容损坏 → 可读错误
        let dir = store.dir().join("pages");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mh0000000004.json"), "{ 不是 JSON").unwrap();
        let error = store.load_page("mh0000000004").unwrap_err();
        assert_eq!(error.code(), "local_data");

        // 索引损坏 → 按未镜像处理（不报错，重镜像即可）
        std::fs::write(store.dir().join("mirror.json"), "坏索引").unwrap();
        assert_eq!(store.load_index().pages(), 0);
    }

    #[test]
    fn load_pages_reads_every_indexed_page() {
        let store = store("load-pages");
        assert!(store.load_pages().unwrap().is_empty(), "没镜像时应为空");

        let page = page("mh0000000007", None);
        store.save_page(&page).unwrap();
        store
            .save_index(&with_page(store.load_index(), &page))
            .unwrap();

        let pages = store.load_pages().unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].path_id, "mh0000000007");
    }

    #[test]
    fn schema_drift_is_treated_as_missing() {
        let store = store("schema");
        let mut page = page("mh0000000005", None);
        page.schema_version = PAGE_SCHEMA_VERSION + 1;
        store.save_page(&page).unwrap();
        assert!(store.load_page("mh0000000005").unwrap().is_none());

        let mut index = MirrorIndex::default();
        index.schema_version += 1;
        store.save_index(&index).unwrap();
        assert_eq!(store.load_index().pages(), 0);
    }

    #[test]
    fn counts_cover_table_images() {
        let mut page = page("mh0000000006", None);
        page.elements.push(PageElement::Table {
            columns: vec![100],
            rows: vec![vec![crate::page::PageCell {
                runs: Vec::new(),
                images: vec!["https://act-webstatic.mihoyo.com/x.png".to_owned()],
                colspan: 1,
                rowspan: 1,
                header: false,
            }]],
        });
        assert_eq!(page.tables(), 1);
        assert_eq!(page.images(), 1, "表格里的图也要计入规模");
    }

    #[tokio::test]
    async fn completed_items_advance_progress_one_by_one() {
        let store = store("progress");
        let first = page("mh0000000010", Some("v1"));
        let second = page("mh0000000011", Some("v1"));
        let index = with_page(with_page(MirrorIndex::default(), &first), &second);
        store.save_index(&index).unwrap();
        let docs = vec![
            doc("mh0000000010", Some("v1")),
            doc("mh0000000011", Some("v1")),
        ];
        let mut reports = Vec::new();

        let (run, _) = mirror_all(
            &store,
            &docs,
            MirrorOptions::default(),
            |report| reports.push(report),
            || false,
        )
        .await
        .unwrap();

        assert_eq!(run.skipped, 2);
        assert_eq!(
            reports
                .iter()
                .map(|report| (report.done, report.total))
                .collect::<Vec<_>>(),
            vec![(1, 2), (2, 2)]
        );
    }
}
