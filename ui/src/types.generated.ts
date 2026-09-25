// Generated from Rust by examples/generate_bindings.rs. Do not edit.
export type KnowledgeStatus = {
/**
 * 正文索引是否已就绪（目录已存在但没有页面镜像时仍为 false）。
 */
ready: boolean,
/**
 * 缓存目录（与 MCP 二进制共享同一份）。
 */
cache_dir: string,
/**
 * 抓取时刻的 Unix 秒；前端按本地时区渲染。
 *
 * 与 `Account::updated_at` 同口径给 ts-rs 标注类型，否则 `i64` 会生成 `bigint`；
 * 这一个是可空的（没抓过就是 `null`），所以连同可空一起写进标注。
 */
cached_at: number | null, age_days: number | null,
/**
 * 是否已超过新鲜度阈值（超过仍可用，只是提示可刷新）。
 */
stale: boolean, schema_version: number, docs: number, leaf_docs: number, chars: number, blocks: number, nodes: number, nodes_server: number, nodes_client: number,
/**
 * 含结构化表格的块数（表格结构已由页面镜像带出来，不再是"待补"）。
 */
table_blocks: number,
/**
 * 缓存不可用时的可读原因（界面原样展示）。
 */
error: string | null,
/**
 * 现成的 MCP 配置片段；二进制不存在时仍给出模板并附提示。
 */
mcp_config: string,
/**
 * MCP 二进制的绝对路径；找不到时为 `None`。
 */
mcp_binary: string | null, };
export type KnowledgeEmbeddingStatus = { active: boolean, error: string | null, models: Array<KnowledgeModelInfo>, };
export type KnowledgeModelInfo = { name: string, path: string, supported: boolean, missing_files: Array<string>, };
export type KnowledgeHit = { path_id: string, doc_title: string,
/**
 * 目录面包屑 + 标题层级路径（消歧的关键字段）。
 */
breadcrumb: Array<string>, anchor: string,
/**
 * `server` / `client` / `both`（非节点块为空）。
 */
side: string | null,
/**
 * 所属节点图类别。
 */
node_graph: string | null,
/**
 * 该篇最后更新时间（官方给的字符串，原样展示）。
 */
updated_at: string | null, url: string, text: string,
/**
 * 命中来源：`candidate_node` / `candidate_doc` / `sparse`。
 */
kind: string, table: Array<Array<string>>, };
export type KnowledgeNodeCandidate = { name: string, path_id: string, doc_title: string, breadcrumb: Array<string>, anchor: string,
/**
 * 节点归属端（`server` / `client` / `both`；非节点块为空）。
 */
side: string | null, node_graph: string | null,
/**
 * 该节点的参数表（来自页面镜像里的表格，含表头行）；镜像里没有表格时为空。
 */
params: Array<Array<string>>,
/**
 * 参数表缺失（镜像里没有表格，需要回官方原文看）。
 */
params_need_render: boolean, url: string, };
export type KnowledgeDocBrief = { path_id: string, title: string, breadcrumb: Array<string>, section: string, is_leaf: boolean, updated_at: string | null, };
export type KnowledgeDocGroup = { keyword: string, total: number, docs: Array<KnowledgeDocBrief>, };
export type KnowledgeDocText = { path_id: string, title: string, url: string, updated_at: string | null, side: string | null, node_graph: string | null,
/**
 * 该篇是否含结构化表格（镜像已带出表格结构）。
 */
has_tables: boolean, blocks: Array<KnowledgeHit>, };
export type KnowledgeSearchGroup = { query: string, hits: Array<KnowledgeHit>, };
export type KnowledgeNodeGroup = { query: string, matches: Array<KnowledgeNodeCandidate>, };
export type KnowledgeRefreshReport = { documents: number, bytes: number, status: KnowledgeStatus, };
export type KnowledgeMirrorStatus = {
/**
 * 已镜像的篇数（含官方已更新、需要重镜像的）。
 */
mirrored: number,
/**
 * 应镜像的篇数（catalog 全部节点；目录页自身也有正文）。
 */
total: number,
/**
 * 官方 `updated_at` 变过、需要重新镜像的篇数。
 */
outdated: number, elements: number, images: number,
/**
 * 是否有镜像作业正在跑。
 */
running: boolean,
/**
 * 最近一篇的进度（进度条用）；从来没跑过为 `None`。
 */
last: MirrorProgress | null, };
export type MirroredPage = { schema_version: number, path_id: string, title: string, url: string,
/**
 * 镜像时刻对应目录里的 `updated_at`：用它判断要不要重新镜像。
 */
updated_at?: string | null, mirrored_at: number, elements: Array<PageElement>, };
export type PageElement = { "type": "heading", level: number, text: string, anchor: string, } | { "type": "paragraph", runs: Array<PageRun>, } | { "type": "list", kind: ListKind,
/**
 * 缩进层级（`data-list-indent`，官方从 1 起）。
 */
indent: number, items: Array<Array<PageRun>>, } | { "type": "table",
/**
 * 每列宽度（px，来自 `colgroup`；缺失为 0，前端自适应）。
 */
columns: Array<number>, rows: Array<Array<PageCell>>, } | { "type": "image", src: string, width?: number | null, } | { "type": "divider" };
export type PageRun = { text: string, bold?: boolean, italic?: boolean,
/**
 * 官方正文里的着色标注，原样保留（前端直接当 CSS 颜色用）。
 */
color?: string | null,
/**
 * 链接：站内相对路径补成官方绝对地址，外链原样。
 */
link?: string | null, };
export type PageCell = { runs: Array<PageRun>,
/**
 * 单元格里的图片（「单位状态效果池」这类表格的示意列，可能不止一张）。
 */
images?: Array<string>, colspan: number, rowspan: number,
/**
 * 是否表头单元格（前端渲染成 `th`）。
 */
header?: boolean, };
export type ListKind = "bullet" | "ordered";
export type MirrorProgress = { path_id: string, title: string,
/**
 * 已处理篇数（含跳过与失败）。
 */
done: number, total: number,
/**
 * 当前索引里已有多少篇；成功落盘后立即更新，供前端摘要同步递增。
 */
mirrored: number, elements: number, tables: number, images: number,
/**
 * 这一篇是否因"已是新的"被跳过（此时只有 `done` / `total` 有意义）。
 */
skipped?: boolean,
/**
 * 这一篇失败的原因；成功与跳过为 `None`。
 */
failed?: string | null, };
export type MirrorRun = { mirrored: number, skipped: number, failed: number, };
