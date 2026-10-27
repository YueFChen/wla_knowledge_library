//! 页面镜像：把**渲染后的正文**还原成结构化元素（调研 §13.5「内容保真度」）。
//!
//! 为什么要有它：静态三件套里的正文是压平的连续文字——表格没有列边界、图片只剩"如图"，
//! 前端不可能据此还原官方页面的排版。渲染通道（[`crate::render`]）拿到的 DOM 里这些结构都在，
//! 这个模块负责把它们抽成与官方页面一一对应的元素：
//!
//! | DOM（2026-09-20 实测） | 元素 |
//! | --- | --- |
//! | `h1` / `h2` / `h3`（带 `id` 锚点） | [`PageElement::Heading`] |
//! | `p`（含 `strong` / `em` / 着色 `span` / `a` / `br`） | [`PageElement::Paragraph`] |
//! | `div.table-wrapper > table`（`colgroup` 列宽 + 单元格 `colspan` / `rowspan`） | [`PageElement::Table`] |
//! | `div.prosemirror-list-new`（每项一个，同级连续） | [`PageElement::List`] |
//! | `img.lazy`（`src` 或 `data-src`，与官方静态资源同域） | [`PageElement::Image`] |
//! | `video` / `iframe`（媒体 URL） | 指向原视频的链接，不下载媒体文件 |
//! | `div.split-line` | [`PageElement::Divider`] |
//!
//! 三条边界：
//! - **图片只记 URL、不下载**（2026-09-20 决定）：实测截图类单图 0.5–2 MB，全量会到 GB 级；
//!   界面直接引官方 CDN，离线时留占位。这里只做"地址是否可信"的校验。
//! - **官方把一个列表拆成同级连续的多个 `div.prosemirror-list-new`**（每项一个），
//!   因此相邻且「类型 + 缩进」相同的列表会并成一个 [`PageElement::List`]。
//! - **只认官方页面的写法**：class 名与 `data-list-*` 属性都按实测的官方 DOM 来；
//!   官改版式时退化为"少几个元素"，不 panic、不猜。

use serde::{Deserialize, Serialize};

use crate::dom::{Child, Node, parse};
use crate::model::SITE_ROOT;

/// 镜像产物的结构版本；参与缓存校验，元素含义或提取规则变化时必须递增。
pub const PAGE_SCHEMA_VERSION: u32 = 2;

/// 一段连续文本及其内联样式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct PageRun {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// 官方正文里的着色标注，原样保留（前端直接当 CSS 颜色用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// 链接：站内相对路径补成官方绝对地址，外链原样。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

impl PageRun {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            color: None,
            link: None,
        }
    }
}

/// 表格单元格。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
pub struct PageCell {
    pub runs: Vec<PageRun>,
    /// 单元格里的图片（「单位状态效果池」这类表格的示意列，可能不止一张）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    #[serde(default = "one")]
    pub colspan: u32,
    #[serde(default = "one")]
    pub rowspan: u32,
    /// 是否表头单元格（前端渲染成 `th`）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub header: bool,
}

/// 列表类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    Bullet,
    Ordered,
}

/// 页面元素；顺序即官方页面里的出现顺序。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PageElement {
    Code {
        language: String,
        text: String,
    },
    /// 标题；`anchor` 是官方页面里的标题 `id`，用于跳回原文。
    Heading {
        level: u8,
        text: String,
        anchor: String,
    },
    Paragraph {
        runs: Vec<PageRun>,
    },
    List {
        kind: ListKind,
        /// 缩进层级（`data-list-indent`，官方从 1 起）。
        indent: u8,
        items: Vec<Vec<PageRun>>,
    },
    Table {
        /// 每列宽度（px，来自 `colgroup`；缺失为 0，前端自适应）。
        columns: Vec<u32>,
        rows: Vec<Vec<PageCell>>,
    },
    /// 正文图片：只记地址与显示宽度，字节不落盘（见模块注释）。
    Image {
        src: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<u32>,
    },
    /// 视频源链接；镜像保存链接，不下载视频内容。
    VideoLink {
        url: String,
    },
    Divider,
}

impl PageElement {
    /// 标题元素 →（层级, 文本, 锚点），供目录使用。
    pub fn heading(&self) -> Option<(u8, &str, &str)> {
        match self {
            Self::Heading {
                level,
                text,
                anchor,
            } => Some((*level, text.as_str(), anchor.as_str())),
            _ => None,
        }
    }
}

fn one() -> u32 {
    1
}

/// 从渲染后的 DOM 里抽出正文元素；找不到正文容器时返回空表。
pub fn elements_from_dom(html: &str) -> Vec<PageElement> {
    let root = parse(html);
    let Some(container) = root.find(|node| node.has_class("doc-view")) else {
        return Vec::new();
    };
    let mut elements = Vec::new();
    walk(container, &mut elements);
    elements
}

/// 遍历块级子节点；直接挂在容器下的散文本按段落收口。
fn walk(node: &Node, out: &mut Vec<PageElement>) {
    let mut loose: Vec<PageRun> = Vec::new();
    for child in &node.children {
        let block = match child {
            Child::Text(text) => {
                push_text(&mut loose, text, Marks::default());
                continue;
            }
            Child::Node(node) => node,
        };
        match block.tag.as_str() {
            "h1" | "h2" | "h3" | "h4" => {
                flush(&mut loose, out);
                let text = block.text();
                if !text.is_empty() {
                    out.push(PageElement::Heading {
                        level: block.tag[1..].parse::<u8>().unwrap_or(1),
                        text,
                        anchor: block.attr("id").unwrap_or_default().to_owned(),
                    });
                }
            }
            "p" => {
                // 官方正文一个 <p> 就是一行，逐段落块（不跨段落合并）。
                flush(&mut loose, out);
                out.extend(inline(block));
            }
            "table" => {
                flush(&mut loose, out);
                if let Some(table) = table(block) {
                    out.push(table);
                }
            }
            "br" => push_text(&mut loose, "\n", Marks::default()),
            "img" => {
                flush(&mut loose, out);
                if let Some(image) = image(block) {
                    out.push(image);
                }
            }
            "video" | "iframe" => {
                flush(&mut loose, out);
                if let Some(link) = video_link(block) {
                    out.push(link);
                }
            }
            "svg" | "script" | "style" => {}
            _ if block.has_class("table-wrapper") => {
                flush(&mut loose, out);
                if let Some(table) = table(block) {
                    out.push(table);
                }
            }
            _ if block.has_class("prosemirror-list-new") => {
                flush(&mut loose, out);
                push_list(block, out);
            }
            _ if block.has_class("split-line") => {
                flush(&mut loose, out);
                out.push(PageElement::Divider);
            }
            // 其余容器：里面还有块级内容就继续按块走，纯内联（卡片链接这类）就按段落收。
            _ => {
                flush(&mut loose, out);
                if has_block(block) {
                    walk(block, out);
                } else {
                    out.extend(inline(block));
                }
            }
        }
    }
    flush(&mut loose, out);
}

/// 是否含块级子内容（判"继续按块走"还是"当段落收"）。
fn has_block(node: &Node) -> bool {
    node.children_nodes().any(|child| {
        matches!(
            child.tag.as_str(),
            "p" | "div"
                | "table"
                | "section"
                | "img"
                | "video"
                | "iframe"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
        ) || has_block(child)
    })
}

/// 走一个内联容器，按顺序产出段落、图片与视频链接。
fn inline(node: &Node) -> Vec<PageElement> {
    let mut runs: Vec<PageRun> = Vec::new();
    let mut out: Vec<PageElement> = Vec::new();
    collect(node, Marks::default(), &mut runs, &mut out);
    flush(&mut runs, &mut out);
    out
}

/// 内联样式栈：借用 DOM 上的原始值，落到文本时再转成自有字符串。
#[derive(Clone, Copy, Default)]
struct Marks<'a> {
    bold: bool,
    italic: bool,
    color: Option<&'a str>,
    link: Option<&'a str>,
}

fn collect(node: &Node, marks: Marks<'_>, runs: &mut Vec<PageRun>, out: &mut Vec<PageElement>) {
    for child in &node.children {
        let node = match child {
            Child::Text(text) => {
                push_text(runs, text, marks);
                continue;
            }
            Child::Node(node) => node,
        };
        let mut next = marks;
        match node.tag.as_str() {
            "strong" | "b" => next.bold = true,
            "em" | "i" => next.italic = true,
            "span" => {
                if let Some(color) = node.attr("style").and_then(color_of) {
                    next.color = Some(color);
                }
            }
            "a" => next.link = node.attr("href").and_then(link_of),
            "br" => {
                // 段落内换行：单独一段 "\n"，前端按 `white-space: pre-wrap` 断行。
                push_break(runs);
                continue;
            }
            "img" => {
                flush(runs, out);
                if let Some(image) = image(node) {
                    out.push(image);
                }
                continue;
            }
            "video" | "iframe" => {
                flush(runs, out);
                if let Some(link) = video_link(node) {
                    out.push(link);
                }
                continue;
            }
            // 图标（svg/path）等内联杂项照样递归：它们没有文字，递归进去什么都不会产出。
            _ => {}
        }
        collect(node, next, runs, out);
    }
}

/// 段内换行：单独一段 `"\n"`（`collapse` 会吃掉孤立的换行，所以不走它）。
fn push_break(runs: &mut Vec<PageRun>) {
    runs.push(PageRun::plain("\n"));
}

fn push_text(runs: &mut Vec<PageRun>, raw: &str, marks: Marks<'_>) {
    let text = collapse(raw);
    if text.is_empty() {
        return;
    }
    runs.push(PageRun {
        text,
        bold: marks.bold,
        italic: marks.italic,
        color: marks.color.map(str::to_owned),
        link: marks.link.map(absolute_link),
    });
}

fn flush(runs: &mut Vec<PageRun>, out: &mut Vec<PageElement>) {
    if !runs.is_empty() {
        out.push(PageElement::Paragraph {
            runs: std::mem::take(runs),
        });
    }
}

/// 空白压缩：保留单个换行（`br`），其余压成空格。
fn collapse(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut spaced = false;
    for c in raw.chars() {
        if c == '\n' {
            out.push('\n');
            spaced = false;
        } else if c.is_whitespace() {
            if !spaced && !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
                spaced = true;
            }
        } else {
            out.push(c);
            spaced = false;
        }
    }
    out.trim_end().to_owned()
}

/// 表格：列宽、单元格与 `colspan` / `rowspan`。
fn table(wrapper: &Node) -> Option<PageElement> {
    let table = if wrapper.tag == "table" {
        wrapper
    } else {
        wrapper.find(|node| node.tag == "table")?
    };
    let columns: Vec<u32> = table
        .find(|node| node.tag == "colgroup")
        .map(|group| {
            group
                .children_nodes()
                .filter(|node| node.tag == "col")
                .map(|col| col.attr("style").and_then(width_of).unwrap_or(0))
                .collect()
        })
        .unwrap_or_default();

    let body = table.find(|node| node.tag == "tbody").unwrap_or(table);
    let mut rows: Vec<Vec<PageCell>> = body
        .children_nodes()
        .filter(|node| node.tag == "tr")
        .map(|row| {
            row.children_nodes()
                .filter(|cell| matches!(cell.tag.as_str(), "td" | "th"))
                .map(|cell| {
                    let (runs, images) = cell_content(cell);
                    PageCell {
                        runs,
                        images,
                        colspan: span(cell, "colspan"),
                        rowspan: span(cell, "rowspan"),
                        header: cell.tag == "th",
                    }
                })
                .collect()
        })
        .filter(|row: &Vec<PageCell>| !row.is_empty())
        .collect();

    if rows.is_empty() {
        return None;
    }
    // 官方表格都是「首行表头」；只有一行的表格可能只是提示框，不当表头处理。
    if rows.len() >= 2 {
        for cell in &mut rows[0] {
            cell.header = true;
        }
    }
    Some(PageElement::Table { columns, rows })
}

/// 单元格内容：文字走内联，图片单独收（「示意」列整列都是图），视频转换成链接文本。
fn cell_content(cell: &Node) -> (Vec<PageRun>, Vec<String>) {
    let mut runs: Vec<PageRun> = Vec::new();
    let mut blocks: Vec<PageElement> = Vec::new();
    collect(cell, Marks::default(), &mut runs, &mut blocks);
    flush(&mut runs, &mut blocks);
    let mut images = Vec::new();
    for block in blocks {
        match block {
            PageElement::Paragraph { runs: more } => runs.extend(more),
            PageElement::Image { src, .. } => images.push(src),
            PageElement::VideoLink { url } => runs.push(link_run(url)),
            _ => {}
        }
    }
    (runs, images)
}

fn span(cell: &Node, name: &str) -> u32 {
    cell.attr(name)
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (1..=64).contains(value))
        .unwrap_or(1)
}

/// 列表：官方每项一个 `div.prosemirror-list-new`，同级连续的会并成一个元素。
fn push_list(node: &Node, out: &mut Vec<PageElement>) {
    let kind = if node.attr("data-list-kind") == Some("ordered") {
        ListKind::Ordered
    } else {
        ListKind::Bullet
    };
    let indent = node
        .attr("data-list-indent")
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(1);
    let items = list_items(node, &mut Vec::new());

    // 与上一个元素同类同缩进 → 并入它（一个列表在 DOM 里是连续多个节点）。
    if let Some(PageElement::List {
        kind: last_kind,
        indent: last_indent,
        items: last_items,
    }) = out.last_mut()
        && *last_kind == kind
        && *last_indent == indent
    {
        last_items.extend(items);
        return;
    }
    out.push(PageElement::List {
        kind,
        indent,
        items,
    });
}

/// 取一个列表节点里的各项文本；嵌套列表（缩进更深）会被就地展开成后续元素。
fn list_items(node: &Node, nested: &mut Vec<PageElement>) -> Vec<Vec<PageRun>> {
    let mut items = Vec::new();
    for child in &node.children {
        let Child::Node(child) = child else {
            continue;
        };
        if child.has_class("list-content") {
            let mut runs: Vec<PageRun> = Vec::new();
            let mut blocks: Vec<PageElement> = Vec::new();
            collect(child, Marks::default(), &mut runs, &mut blocks);
            flush(&mut runs, &mut blocks);
            let mut text: Vec<PageRun> = Vec::new();
            for block in blocks {
                match block {
                    PageElement::Paragraph { runs } => text.extend(runs),
                    PageElement::VideoLink { url } => text.push(link_run(url)),
                    other => nested.push(other),
                }
            }
            items.push(text);
        } else if child.has_class("prosemirror-list-new") {
            let deeper = list_items(child, nested);
            if !deeper.is_empty() {
                nested.push(PageElement::List {
                    kind: if child.attr("data-list-kind") == Some("ordered") {
                        ListKind::Ordered
                    } else {
                        ListKind::Bullet
                    },
                    indent: child
                        .attr("data-list-indent")
                        .and_then(|value| value.parse::<u8>().ok())
                        .unwrap_or(2),
                    items: deeper,
                });
            }
        }
    }
    items
}

/// 正文图片：只保留可信地址（https + 米哈游域），并带上显示宽度。
fn image(node: &Node) -> Option<PageElement> {
    let src = node
        .attr("src")
        .filter(|value| is_trusted_image(value))
        .or_else(|| {
            node.attr("data-src")
                .filter(|value| is_trusted_image(value))
        })?;
    Some(PageElement::Image {
        src: src.to_owned(),
        width: node.attr("style").and_then(width_of),
    })
}

/// 视频只保留可点击的源地址，不镜像媒体数据；兼容 video 的 `src` 和 source 子节点。
fn video_link(node: &Node) -> Option<PageElement> {
    let src = node
        .attr("src")
        .or_else(|| node.attr("data-src"))
        .or_else(|| {
            node.find(|child| child.tag == "source")
                .and_then(|source| source.attr("src").or_else(|| source.attr("data-src")))
        })
        .filter(|value| link_of(value).is_some())?;
    Some(PageElement::VideoLink {
        url: absolute_link(src),
    })
}

fn link_run(url: String) -> PageRun {
    PageRun {
        text: "打开视频链接".to_owned(),
        bold: false,
        italic: false,
        color: None,
        link: Some(url),
    }
}

/// 图片地址白名单：`https` + `*.mihoyo.com`（官方正文图与静态资源同域，调研 §13.11）。
fn is_trusted_image(src: &str) -> bool {
    let Some(rest) = src.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    host.ends_with(".mihoyo.com")
}

/// `style` 里的颜色：只认 `rgb(...)` / `rgba(...)` / `#...`，其余丢弃（避免样式注入）。
fn color_of(style: &str) -> Option<&str> {
    let value = style
        .split(';')
        .find_map(|part| part.trim().strip_prefix("color:"))?
        .trim();
    let head = value.chars().next()?;
    let allowed = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '(' | ')' | ',' | '.' | '%' | ' ' | '#'));
    (allowed && (head == '#' || value.starts_with("rgb"))).then_some(value)
}

/// `style` 里的宽度（如 `width: 498px;`）。
fn width_of(style: &str) -> Option<u32> {
    let part = style
        .split(';')
        .find(|part| part.trim().starts_with("width:"))?;
    let digits: String = part
        .trim()
        .trim_start_matches("width:")
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=100_000).contains(value))
}

/// 链接：只认 http(s) 与站内相对路径（相对路径补成官方绝对地址）。
fn link_of(href: &str) -> Option<&str> {
    (href.starts_with("http://") || href.starts_with("https://") || href.starts_with('/'))
        .then_some(href)
}

/// 站内相对路径 → 官方绝对地址；外链原样返回。
fn absolute_link(href: &str) -> String {
    if href.starts_with('/') {
        return format!("{SITE_ROOT}{href}");
    }
    href.to_owned()
}
