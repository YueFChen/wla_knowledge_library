//! 数据模型：输入（官方目录 + 页面镜像）与产物（结构化文档 / 块 / 节点条目）。
//!
//! 口径来自[知识库早期调研与评测](../../../docs/知识库早期调研与评测.md) §3.2 与 §13.11；
//! `SCHEMA_VERSION` 参与缓存校验（跨进程共享同一份缓存目录）。

use serde::{Deserialize, Serialize};

/// 产物结构版本。字段含义或切块规则变化时必须递增，缓存据此判定失效。
///
/// 2：内容源改为页面镜像——块与标题锚点都从镜像来，`bundle.json` 只留 `catalog.json`，
/// `headingsMap.json` / `textMap.json` 不再需要。
pub const SCHEMA_VERSION: u32 = 2;

/// 节点内容小于该字数视为空节点 / 目录页，不建节点条目。
pub const UNIT_MIN_CHARS: usize = 20;

/// 没有页面镜像时的可读说明（GUI 与 MCP 用同一句话，口径一致）。
pub const NO_MIRROR: &str = "本地还没有页面镜像，请先在知识库页镜像";

/// 目录所在分区基址（知识库 = 综合指南）。
pub const KNOWLEDGE_BASE_URL: &str = "https://act-webstatic.mihoyo.com/ugc-tutorial/knowledge/cn";

/// 语种段（站点路径形如 `/cn/{lang}/`）。
pub const DEFAULT_LANG: &str = "zh-cn";

/// 官方原文链接前缀（文档级深层链接）。
pub const DETAIL_URL: &str = "https://act.mihoyo.com/ys/ugc/tutorial/detail";

/// 站点根：正文里的站内相对链接（如 `/ys/ugc/tutorial/detail/…`）补它。
pub const SITE_ROOT: &str = "https://act.mihoyo.com";

// ── 输入：catalog.json ──────────────────────────────────────────────────────

/// `catalog.json` 的一个节点：带 `children` 即目录页，空数组即文档页。
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogNode {
    pub title: String,
    /// 文档标识，等于站点 URL `detail/{path_id}` 的末段。
    pub path_id: String,
    /// 正文键；302/302 与 `path_id` 相同（调研 §3.2），仍原样读取以便容忍官方变化。
    #[serde(default)]
    pub real_id: Option<String>,
    /// 该篇最后更新时间，增量重建依据。
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub doc_type: Option<String>,
    /// 部分篇目为 `8`，含义未知（调研 §3.2），仅原样保留。
    #[serde(default)]
    pub article_type: Option<i64>,
    #[serde(default)]
    pub children: Vec<CatalogNode>,
}

// ── 产物 ───────────────────────────────────────────────────────────────────

/// 节点归属端（由 catalog 的「服务器节点 / 客户端节点」分区直接推导，调研 §13.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Server,
    Client,
    Both,
}

/// 重建后的文档。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Doc {
    pub path_id: String,
    pub title: String,
    /// 顶层分区（更新日志 / 读前须知 / 界面介绍 / 概念介绍 / 节点介绍 / 辅助功能 / 附录）。
    pub section: String,
    /// 目录面包屑（含自身），用于消歧。
    pub breadcrumb: Vec<String>,
    /// 是否叶子文档（仅描述目录层级；目录页自身也可能有正文）。
    pub is_leaf: bool,
    pub updated_at: Option<String>,
    /// 所属节点图类别（仅「节点介绍」下）。
    pub node_graph: Option<String>,
    pub side: Option<Side>,
    pub url: String,
}

/// 重建后的块：检索与引用的最小单位。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub path_id: String,
    pub doc_title: String,
    pub section: String,
    /// 目录面包屑 + 文档内标题层级路径（消歧的关键字段）。
    pub breadcrumb: Vec<String>,
    /// 文档内的标题层级路径。
    pub heading_path: Vec<String>,
    /// 块所属标题的锚点（镜像里的 `id`），用于跳回原文。
    pub anchor: String,
    /// 块所属标题的层级。
    pub level: u8,
    /// 块正文；表格块为单元格内容按行拼接，供检索。
    pub text: String,
    pub chars: usize,
    pub node_graph: Option<String>,
    pub side: Option<Side>,
    pub updated_at: Option<String>,
    pub url: String,
    /// 表格块的结构化内容（含表头行）；非表格块为空。检索文本是压平的，结构化内容留在这里。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub table: Vec<Vec<String>>,
}

/// 节点条目（「节点介绍」下的二级标题，调研 §4.1）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEntry {
    pub name: String,
    pub path_id: String,
    pub doc_title: String,
    pub anchor: String,
    pub node_graph: Option<String>,
    pub side: Option<Side>,
    /// 该节点的参数表（来自镜像里的表格元素，含表头行）；镜像里没有表格时为空。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Vec<String>>,
    /// 参数表缺失（节点正文里没有表格，需要回原文看）。
    pub params_need_render: bool,
}

// ── 查询结果 ───────────────────────────────────────────────────────────────

/// 命中来自哪一步：结构化候选，还是稀疏召回。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    CandidateNode,
    CandidateDoc,
    Sparse,
    Vector,
}

/// 一条检索命中。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub path_id: String,
    pub doc_title: String,
    pub anchor: String,
    pub breadcrumb: Vec<String>,
    pub text: String,
    pub side: Option<Side>,
    pub node_graph: Option<String>,
    pub updated_at: Option<String>,
    pub url: String,
    pub kind: HitKind,
    /// 表格块保留行列边界，供检索调用方构造可靠的 RAG 上下文。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub table: Vec<Vec<String>>,
}

/// 内核检索原语可接受的可选目录和内容约束；场景识别与路由由上层插件负责。
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Restrict retrieval to one document path id; used by specialized knowledge tools.
    pub doc_path_id: Option<String>,
    /// 顶层分区或目录/标题路径中的一段，例如「概念介绍」「运动器」「控件」。
    pub section: Option<String>,
    pub side: Option<Side>,
    pub node_graph: Option<String>,
    /// 仅返回表格块，适合 API / 参数字段查询。
    pub tables_only: bool,
}

/// 节点候选过滤条件。
#[derive(Debug, Clone, Default)]
pub struct NodeFilters {
    pub side: Option<Side>,
    pub node_graph: Option<String>,
}

/// `get_node` 的候选：同名节点可能有多份，**必须整组返回**由调用方选择（调研 §13.11）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCandidate {
    pub name: String,
    pub path_id: String,
    pub doc_title: String,
    pub anchor: String,
    pub node_graph: Option<String>,
    pub side: Option<Side>,
    pub breadcrumb: Vec<String>,
    /// 该节点的参数表（来自镜像，含表头行）；镜像里没有表格时为空。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Vec<String>>,
    pub params_need_render: bool,
    pub url: String,
}

/// `get_node(names[])` 的分组结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLookup {
    pub query: String,
    /// 命中为空时给出可读说明，便于模型换词重试。
    pub matches: Vec<NodeCandidate>,
}

/// `list_docs(keywords[])` 的分组结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocBrief {
    pub path_id: String,
    pub title: String,
    pub breadcrumb: Vec<String>,
    pub section: String,
    pub is_leaf: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocListGroup {
    pub keyword: String,
    pub total: usize,
    pub docs: Vec<DocBrief>,
}

/// `read_doc` 的返回：整篇（或指定锚点起）的块序列。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocText {
    pub path_id: String,
    pub title: String,
    pub url: String,
    pub updated_at: Option<String>,
    pub side: Option<Side>,
    pub node_graph: Option<String>,
    /// 该篇是否含结构化表格（表格结构已从镜像带出来，不再是"待补"）。
    pub has_tables: bool,
    pub blocks: Vec<SearchHit>,
}

// ── 索引规模（供 GUI / 健康自检展示） ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeStats {
    pub schema_version: u32,
    pub docs: usize,
    pub leaf_docs: usize,
    /// 已镜像、真正有块的篇数（未镜像的篇目不进索引）。
    pub mirrored_docs: usize,
    pub chars: usize,
    pub blocks: usize,
    /// 含结构化表格的块数。
    pub tables: usize,
    pub nodes: usize,
    /// 参数表已从镜像取到的节点数。
    pub nodes_with_params: usize,
    pub nodes_server: usize,
    pub nodes_client: usize,
}
