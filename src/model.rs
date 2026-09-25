//! 插件对外的 DTO。
//!
//! 全部由 `examples/generate_bindings.rs` 生成到插件自己的 UI 类型文件，随插件一起打包。
//!
//! 与插件内部 `wonderland-knowledge` crate 的类型**刻意分开**：领域 crate 不含 `ts-rs`，
//! 这边是入口 DTO——因此 `side` / `kind` 在这里是字符串，与 MCP 工具返回的取值一致。

use serde::{Deserialize, Serialize};

use crate::MirrorProgress;

/// 本地模型设置、磁盘发现情况与当前进程实际加载状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeEmbeddingStatus {
    pub active: bool,
    pub error: Option<String>,
    pub models: Vec<KnowledgeModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeModelInfo {
    pub name: String,
    pub path: String,
    pub supported: bool,
    pub missing_files: Vec<String>,
}

/// 缓存与索引状态：GUI 的健康自检（能否用、多新、多大、MCP 怎么配）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeStatus {
    /// 正文索引是否已就绪（目录已存在但没有页面镜像时仍为 false）。
    pub ready: bool,
    /// 缓存目录（与 MCP 二进制共享同一份）。
    pub cache_dir: String,
    /// 抓取时刻的 Unix 秒；前端按本地时区渲染。
    ///
    /// 与 `Account::updated_at` 同口径给 ts-rs 标注类型，否则 `i64` 会生成 `bigint`；
    /// 这一个是可空的（没抓过就是 `null`），所以连同可空一起写进标注。
    #[cfg_attr(feature = "bindings", ts(type = "number | null"))]
    pub cached_at: Option<i64>,
    pub age_days: Option<f64>,
    /// 是否已超过新鲜度阈值（超过仍可用，只是提示可刷新）。
    pub stale: bool,
    pub schema_version: u32,
    pub docs: usize,
    pub leaf_docs: usize,
    pub chars: usize,
    pub blocks: usize,
    pub nodes: usize,
    pub nodes_server: usize,
    pub nodes_client: usize,
    /// 含结构化表格的块数（表格结构已由页面镜像带出来，不再是"待补"）。
    pub table_blocks: usize,
    /// 缓存不可用时的可读原因（界面原样展示）。
    pub error: Option<String>,
    /// 现成的 MCP 配置片段；二进制不存在时仍给出模板并附提示。
    pub mcp_config: String,
    /// MCP 二进制的绝对路径；找不到时为 `None`。
    pub mcp_binary: Option<String>,
}

/// 一条命中：检索结果、文档正文块共用同一形状（与 MCP 的 `SearchHit` 取值一致）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeHit {
    pub path_id: String,
    pub doc_title: String,
    /// 目录面包屑 + 标题层级路径（消歧的关键字段）。
    pub breadcrumb: Vec<String>,
    pub anchor: String,
    /// `server` / `client` / `both`（非节点块为空）。
    pub side: Option<String>,
    /// 所属节点图类别。
    pub node_graph: Option<String>,
    /// 该篇最后更新时间（官方给的字符串，原样展示）。
    pub updated_at: Option<String>,
    pub url: String,
    pub text: String,
    /// 命中来源：`candidate_node` / `candidate_doc` / `sparse`。
    pub kind: String,
    /// 表格块的行列数据；用于可靠地呈现参数/API 字段。
    pub table: Vec<Vec<String>>,
}

/// 同名节点的一个候选（整组返回，不替用户挑一个）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeNodeCandidate {
    pub name: String,
    pub path_id: String,
    pub doc_title: String,
    pub breadcrumb: Vec<String>,
    pub anchor: String,
    /// 节点归属端（`server` / `client` / `both`；非节点块为空）。
    pub side: Option<String>,
    pub node_graph: Option<String>,
    /// 该节点的参数表（来自页面镜像里的表格，含表头行）；镜像里没有表格时为空。
    pub params: Vec<Vec<String>>,
    /// 参数表缺失（镜像里没有表格，需要回官方原文看）。
    pub params_need_render: bool,
    pub url: String,
}

/// 文档清单里的一篇（不含正文）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeDocBrief {
    pub path_id: String,
    pub title: String,
    pub breadcrumb: Vec<String>,
    pub section: String,
    pub is_leaf: bool,
    pub updated_at: Option<String>,
}

/// 一次 `list_docs` 的分组结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeDocGroup {
    pub keyword: String,
    pub total: usize,
    pub docs: Vec<KnowledgeDocBrief>,
}

/// 整篇文档（可选从某个锚点起）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeDocText {
    pub path_id: String,
    pub title: String,
    pub url: String,
    pub updated_at: Option<String>,
    pub side: Option<String>,
    pub node_graph: Option<String>,
    /// 该篇是否含结构化表格（镜像已带出表格结构）。
    pub has_tables: bool,
    pub blocks: Vec<KnowledgeHit>,
}

/// 一次检索的分组结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeSearchGroup {
    pub query: String,
    pub hits: Vec<KnowledgeHit>,
}

/// 一次节点查询的分组结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeNodeGroup {
    pub query: String,
    pub matches: Vec<KnowledgeNodeCandidate>,
}

/// 更新一次 catalog 的结果（界面上的「获取目录」/「更新目录」）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeRefreshReport {
    pub documents: usize,
    pub bytes: usize,
    pub status: KnowledgeStatus,
}

/// 页面镜像的总体状态（界面上的镜像面板）。
///
/// 镜像的是**页面元素**（标题 / 段落 / 表格 / 列表 / 图片地址），它同时是前端还原官方排版与
/// 检索索引的唯一内容源：没有镜像就没有可检索的正文（`catalog` 只提供目录树）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct KnowledgeMirrorStatus {
    /// 已镜像的篇数（含官方已更新、需要重镜像的）。
    pub mirrored: usize,
    /// 应镜像的篇数（catalog 全部节点；目录页自身也有正文）。
    pub total: usize,
    /// 官方 `updated_at` 变过、需要重新镜像的篇数。
    pub outdated: usize,
    pub elements: usize,
    pub images: usize,
    /// 是否有镜像作业正在跑。
    pub running: bool,
    /// 最近一篇的进度（进度条用）；从来没跑过为 `None`。
    #[cfg_attr(feature = "bindings", ts(type = "MirrorProgress | null"))]
    pub last: Option<MirrorProgress>,
}
