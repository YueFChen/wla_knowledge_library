//! 核查工具：把本地 dump 的详情页 DOM 抽成页面元素，打印清单与规模。
//!
//! 用途是**核对镜像产物**（元素是否漏、表格列宽是否带上）：
//!
//! ```powershell
//! # 先用 Edge 抓一份 DOM（render 通道），再抽元素
//! cargo run -q -p wonderland-knowledge --example page_dump -- target/dom/exec.html
//! ```
use std::collections::BTreeMap;

use wonderland_knowledge::{PageElement, page::elements_from_dom};

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("用法：page_dump <dom.html>...（DOM 由 Edge --dump-dom 产出）");
        return;
    }
    for path in paths {
        let html = std::fs::read_to_string(&path).expect("读 DOM 文件");
        let elements = elements_from_dom(&html);
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for element in &elements {
            let key = match element {
                PageElement::Heading { .. } => "heading",
                PageElement::Paragraph { .. } => "paragraph",
                PageElement::List { .. } => "list",
                PageElement::Table { .. } => "table",
                PageElement::Image { .. } => "image",
                PageElement::VideoLink { .. } => "video_link",
                PageElement::Divider => "divider",
            };
            *counts.entry(key).or_insert(0) += 1;
        }
        println!("\n{path}：{} 个元素 {counts:?}", elements.len());
        for element in elements.iter().take(4) {
            if let Some((level, text, anchor)) = element.heading() {
                let clipped: String = text.chars().take(24).collect();
                println!("  h{level} {clipped} #{anchor}");
            }
        }
        if let Some(PageElement::Table { columns, rows }) = elements
            .iter()
            .find(|element| matches!(element, PageElement::Table { .. }))
        {
            let first: String = rows[0]
                .iter()
                .map(|cell| {
                    cell.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join(" | ");
            let clipped: String = first.chars().take(60).collect();
            println!(
                "  首个表：{} 列 {columns:?} / {} 行｜首行 {clipped}",
                columns.len(),
                rows.len()
            );
        }
    }
}
