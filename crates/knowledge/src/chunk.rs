//! 重建：官方目录（catalog）+ 页面镜像 → 结构化文档与块。
//!
//! 为什么以镜像为唯一内容源：官方静态语料（`textMap.json`）把正文压平——表格没有列边界、
//! 图片只剩"如图"，参数类问题答不准（调研 §13.5）。页面镜像保留了元素顺序、表格结构与标题锚点，
//! 因此 `textMap` / `headingsMap` 都不再需要，`catalog` 只用来提供目录树（分区 / 标题 / 更新时刻）。
//!
//! 切块口径（按镜像元素顺序）：
//! - 标题只建立层级路径，不单独成块；锚点取该块所属标题的 `anchor`；
//! - 段落与列表项各成一个块；
//! - 表格自成一个块：检索文本是单元格内容按行拼接，结构化内容留在 [`Block::table`] 上。
//!
//! 未镜像的篇目没有块，自然不进检索；文档树仍完整保留（镜像作业要按它列出待镜像篇目）。

use std::collections::HashMap;

use crate::mirror::MirroredPage;
use crate::model::{Block, CatalogNode, DETAIL_URL, Doc, NodeEntry, Side, UNIT_MIN_CHARS};
use crate::page::{PageCell, PageElement, PageRun};

/// 重建结果与本次重建的统计。
pub struct Rebuilt {
    pub docs: Vec<Doc>,
    pub blocks: Vec<Block>,
    pub nodes: Vec<NodeEntry>,
    /// 全部块的字符数（检索语料体量）。
    pub chars: usize,
}

/// 顶层分区标题：这些标题太泛，不做别名（§13.11 的对照实现）。
pub fn top_sections(docs: &[Doc]) -> Vec<String> {
    docs.iter()
        .filter(|d| d.breadcrumb.len() == 1)
        .map(|d| d.title.clone())
        .collect()
}

/// 从 catalog + 一批页面镜像重建。
///
/// 目录里没有的 `path_id`（官方已下架 / 缓存陈旧）直接忽略，不 panic。
pub fn rebuild(catalog: &[CatalogNode], pages: &[MirroredPage]) -> Rebuilt {
    let mut docs = Vec::new();
    for node in catalog {
        walk(node, &[], None, None, &mut docs);
    }
    let by_id: HashMap<&str, &Doc> = docs.iter().map(|d| (d.path_id.as_str(), d)).collect();

    let mut blocks: Vec<Block> = Vec::new();
    let mut nodes: Vec<NodeEntry> = Vec::new();
    let mut chars = 0usize;
    for page in pages {
        let Some(doc) = by_id.get(page.path_id.as_str()) else {
            continue;
        };
        let (mut page_blocks, mut page_nodes, page_chars) = build_page(doc, page);
        blocks.append(&mut page_blocks);
        nodes.append(&mut page_nodes);
        chars += page_chars;
    }

    Rebuilt {
        docs,
        blocks,
        nodes,
        chars,
    }
}

/// 一层标题（层级栈里的一项）。
struct Level {
    level: u8,
    text: String,
    anchor: String,
}

/// 「节点介绍」下的一个二级标题（节点条目的候选）。
struct NodeHead {
    name: String,
    anchor: String,
    /// 该节点所属的标题层级路径（含自身）。
    path: Vec<String>,
}

/// 把一篇镜像按元素顺序切成块，并抽出节点条目。
fn build_page(doc: &Doc, page: &MirroredPage) -> (Vec<Block>, Vec<NodeEntry>, usize) {
    let mut blocks: Vec<Block> = Vec::new();
    let mut node_heads: Vec<NodeHead> = Vec::new();
    // 标题层级栈：缺中间层级时保留"空洞"，标题路径只取实际存在的层级。
    let mut stack: Vec<Option<Level>> = Vec::new();
    let mut chars = 0usize;

    for element in &page.elements {
        match element {
            PageElement::Heading {
                level,
                text,
                anchor,
            } => {
                let text = clean_heading(text);
                chars += text.chars().count();
                if text.is_empty() {
                    continue;
                }
                let level = (*level).max(1) as usize;
                stack.truncate(level - 1);
                while stack.len() < level - 1 {
                    stack.push(None);
                }
                stack.push(Some(Level {
                    level: level as u8,
                    text,
                    anchor: anchor.clone(),
                }));
                // 节点条目：只有「节点介绍」分区下的二级标题才是节点（调研 §4.1）。
                if doc.section == "节点介绍"
                    && level == 2
                    && let Some(head) = stack.iter().flatten().next_back()
                {
                    node_heads.push(NodeHead {
                        name: head.text.clone(),
                        anchor: head.anchor.clone(),
                        path: heading_path(&stack),
                    });
                }
            }
            PageElement::Paragraph { runs } => {
                push(
                    &mut blocks,
                    &mut chars,
                    doc,
                    &stack,
                    runs_text(runs),
                    Vec::new(),
                );
            }
            // 列表项各成一个块：一项通常就是一条独立的操作说明。
            PageElement::List { items, .. } => {
                for item in items {
                    push(
                        &mut blocks,
                        &mut chars,
                        doc,
                        &stack,
                        runs_text(item),
                        Vec::new(),
                    );
                }
            }
            PageElement::Table { rows, .. } => {
                let table = table_rows(rows);
                let text = table_text(&table);
                push(&mut blocks, &mut chars, doc, &stack, text, table);
            }
            // 图片只记地址、分隔线只是排版：都不构成检索块。
            PageElement::Image { .. } | PageElement::Divider => {}
        }
    }

    let nodes = node_heads
        .into_iter()
        .filter_map(|head| node_of(doc, &blocks, head))
        .collect();
    (blocks, nodes, chars)
}

/// 生成一个块；空文本（空白段落）不建块。
fn push(
    blocks: &mut Vec<Block>,
    chars: &mut usize,
    doc: &Doc,
    stack: &[Option<Level>],
    text: String,
    table: Vec<Vec<String>>,
) {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return;
    }
    let heading_path = heading_path(stack);
    let owner = stack.iter().flatten().next_back();
    let mut breadcrumb = doc.breadcrumb.clone();
    breadcrumb.extend(heading_path.iter().cloned());
    let chars_here = text.chars().count();
    *chars += chars_here;
    blocks.push(Block {
        path_id: doc.path_id.clone(),
        doc_title: doc.title.clone(),
        section: doc.section.clone(),
        breadcrumb,
        heading_path,
        anchor: owner.map(|level| level.anchor.clone()).unwrap_or_default(),
        level: owner.map(|level| level.level).unwrap_or(0),
        text,
        chars: chars_here,
        node_graph: doc.node_graph.clone(),
        side: doc.side,
        updated_at: doc.updated_at.clone(),
        url: doc.url.clone(),
        table,
    });
}

/// 由「节点介绍」下的二级标题生成节点条目：内容太少的（"待补"这类空节点）不建。
fn node_of(doc: &Doc, blocks: &[Block], head: NodeHead) -> Option<NodeEntry> {
    // 该节点的内容 = 标题路径以它开头的块（限同一篇文档）。
    let scope: Vec<&Block> = blocks
        .iter()
        .filter(|block| block.heading_path.starts_with(&head.path))
        .collect();
    let content: usize = scope.iter().map(|block| block.chars).sum();
    if content < UNIT_MIN_CHARS {
        return None;
    }
    // 参数表优先取"参数"那张（节点正文里可能还有别的表）。
    let params = scope
        .iter()
        .find(|block| !block.table.is_empty() && block.text.contains("参数"))
        .or_else(|| scope.iter().find(|block| !block.table.is_empty()))
        .map(|block| block.table.clone())
        .unwrap_or_default();
    Some(NodeEntry {
        name: head.name,
        path_id: doc.path_id.clone(),
        doc_title: doc.title.clone(),
        anchor: head.anchor,
        node_graph: doc.node_graph.clone(),
        side: doc.side,
        params_need_render: params.is_empty(),
        params,
    })
}

/// 当前标题层级路径（只剩实际存在的层级）。
fn heading_path(stack: &[Option<Level>]) -> Vec<String> {
    stack
        .iter()
        .flatten()
        .map(|level| level.text.clone())
        .collect()
}

/// 内联片段拼成纯文本（片段本就是同一行的连续文字，直接相接）。
fn runs_text(runs: &[PageRun]) -> String {
    runs.iter()
        .map(|run| run.text.as_str())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// 表格 → 行 × 列的纯文本（结构化内容保留在块上）。
fn table_rows(rows: &[Vec<PageCell>]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|cell| runs_text(&cell.runs)).collect())
        .collect()
}

/// 表格的检索文本：单元格内容按行拼接（行内以空格分隔）。
fn table_text(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(String::as_str)
                .filter(|cell| !cell.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 递归建树：按 catalog 的深度决定顶层分区与「节点介绍」下的归属端。
fn walk(
    node: &CatalogNode,
    ancestors: &[String],
    section: Option<&str>,
    side: Option<(String, Side)>,
    docs: &mut Vec<Doc>,
) {
    let title = node.title.clone();
    let section = match section {
        Some(section) => section.to_owned(),
        None => title.clone(),
    };
    // 「节点介绍」的第二层就是节点图分区：服务器节点 / 客户端节点。
    let side = if section == "节点介绍" && ancestors.len() == 1 {
        let derived = if title.contains("客户端") {
            Side::Client
        } else if title.contains("服务端") || title.contains("服务器") {
            Side::Server
        } else {
            Side::Both
        };
        Some((clean_heading(&title), derived))
    } else {
        side
    };

    let path_id = node.path_id.clone();
    let mut breadcrumb = ancestors.to_vec();
    breadcrumb.push(title.clone());
    docs.push(Doc {
        path_id: path_id.clone(),
        title,
        section: section.clone(),
        breadcrumb: breadcrumb.clone(),
        is_leaf: node.children.is_empty(),
        updated_at: node.updated_at.clone(),
        node_graph: side.as_ref().map(|(graph, _)| graph.clone()),
        side: side.as_ref().map(|(_, s)| *s),
        url: format!("{DETAIL_URL}/{path_id}"),
    });

    for child in &node.children {
        walk(child, &breadcrumb, Some(&section), side.clone(), docs);
    }
}

/// 标题清洗：去 markdown 标记与序号前缀（「1.添加组件」→「添加组件」），
/// 但「7.0版本」这类"数字 + 点 + 数字"的标题不能被误剥。
pub fn clean_heading(raw: &str) -> String {
    let stripped: String = raw
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '`'))
        .collect();
    let mut rest = stripped.trim();
    while let Some(next) = strip_one_ordinal(rest) {
        rest = next.trim();
    }
    rest.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 剥掉一层序号前缀；不匹配时返回 `None`。
fn strip_one_ordinal(text: &str) -> Option<&str> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    // 形式一：`（1）` / `(1)`
    if matches!(first, '（' | '(') {
        let close_at = text.find(['）', ')'])?;
        let inner = &text[first.len_utf8()..close_at];
        if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
            return Some(&text[close_at + text[close_at..].chars().next()?.len_utf8()..]);
        }
        return None;
    }
    // 形式二：`1.` / `1、`，且分隔符后不是数字
    if !first.is_ascii_digit() {
        return None;
    }
    let digits_end = text
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)?;
    let tail = text[digits_end..].trim_start_matches([' ', '\t', '\u{3000}']);
    let after_sep = tail.strip_prefix(['.', '、'])?;
    if after_sep.starts_with(|c: char| c.is_ascii_digit()) {
        return None; // 「7.0版本」：保住原样
    }
    Some(after_sep)
}

/// 判断 `needle` 是否是 `haystack` 的子序列（官方标题常被拆开写，子序列匹配能救回来）。
pub fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|c| chars.any(|h| h == c))
}
