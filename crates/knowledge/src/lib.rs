//! 知识库领域库：目录 + 页面镜像 → 结构化块 → 稀疏检索 / 别名表 / 结构化查询。
//!
//! 定位：**协议、进程与 Tauri 全无关**。基础库提供普通检索、结构化查询与过滤能力；
//! 桌面插件负责场景规划，MCP 适配器复用插件规划并把结果转换成协议响应。
//!
//! 内容源只有一个：**页面镜像**（[`mirror`]，保留元素顺序、表格结构与标题锚点）。
//! `catalog.json` 仍要抓，但它只提供目录树；`headingsMap.json` / `textMap.json` 已不再需要。
//!
//! 分工：
//! - [`chunk`]：重建（目录 + 页面镜像 → 文档 / 块 / 节点条目）；
//! - [`index`]：检索与四组只读查询（`search` / `get_node` / `list_docs` / `read_doc`）；
//! - [`render`]：渲染通道（Edge 无头 dump DOM），供 [`mirror`] 抽取页面元素；
//! - [`cache`]：`catalog.json` 的本地留存、原子替换与新鲜度判断（GUI 与 MCP 进程共享）；
//! - 目录同步：由桌面插件经 Core 的受控网络能力完成，基础库不直接联网。
//!
//! 尚未实现（下一步，见调研 §7.4 / §12.5）：词典表（人工同义词）、图片按需下载。

pub mod cache;
mod chunk;
pub mod dom;
mod index;
pub mod mirror;
mod model;
pub mod page;
pub mod render;
mod vector;

#[cfg(test)]
mod page_tests;
#[cfg(test)]
mod tests;

pub use chunk::{Rebuilt, clean_heading, is_subsequence, rebuild, top_sections};
pub use index::KnowledgeBase;
pub use mirror::{
    MirrorIndex, MirrorOptions, MirrorProgress, MirrorRun, MirrorStore, MirroredPage, mirror_all,
    mirror_one, render_page,
};
pub use model::*;
pub use page::{ListKind, PAGE_SCHEMA_VERSION, PageCell, PageElement, PageRun};
pub use vector::{
    LOCAL_EMBEDDING_MODEL_DIR, LOCAL_EMBEDDING_MODEL_FILES, LOCAL_EMBEDDING_MODEL_ID,
};

use tracing::info;
use wonderland_plugin_sdk::PluginFailure;

/// 从 `catalog.json` 原文 + 一批页面镜像重建并建索引（示例、测试与缓存读取共用；不联网）。
///
/// 镜像为空时返回的是**空索引**（没有块、没有节点）；"还没有镜像，请先镜像"这句话由入口给出，
/// 因为只有入口知道界面该引导用户做什么（见 [`NO_MIRROR`]）。
pub fn build_from_json(
    catalog_json: &str,
    pages: &[MirroredPage],
) -> Result<KnowledgeBase, PluginFailure> {
    let catalog: Vec<CatalogNode> = serde_json::from_str(catalog_json)
        .map_err(|error| PluginFailure::LocalData(format!("catalog.json 解析失败：{error}")))?;
    let rebuilt = rebuild(&catalog, pages);
    info!(
        docs = rebuilt.docs.len(),
        blocks = rebuilt.blocks.len(),
        nodes = rebuilt.nodes.len(),
        "知识库重建完成"
    );
    Ok(KnowledgeBase::new(rebuilt))
}
