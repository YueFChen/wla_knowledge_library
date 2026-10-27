// Generated from Rust by examples/generate_bindings.rs. Do not edit.
export type PageElement = { "type": "code", language: string, text: string, } | { "type": "heading", level: number, text: string, anchor: string, } | { "type": "paragraph", runs: Array<PageRun>, } | { "type": "list", kind: ListKind,
/**
 * 缩进层级（`data-list-indent`，官方从 1 起）。
 */
indent: number, items: Array<Array<PageRun>>, } | { "type": "table",
/**
 * 每列宽度（px，来自 `colgroup`；缺失为 0，前端自适应）。
 */
columns: Array<number>, rows: Array<Array<PageCell>>, } | { "type": "image", src: string, width?: number | null, } | { "type": "video_link", url: string, } | { "type": "divider" };
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
