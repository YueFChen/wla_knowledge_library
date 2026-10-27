//! General text formats use a standards-based HTML parser; Wonderland keeps its own adapter.
use crate::{ListKind, PageCell, PageElement, PageRun};
use scraper::{ElementRef, Html, Selector};
use url::Url;

fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("static CSS selector")
}

pub fn decode(bytes: &[u8], charset: Option<&str>) -> Result<String, String> {
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string());
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return Ok(encoding_rs::UTF_16LE.decode(bytes).0.into_owned());
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return Ok(encoding_rs::UTF_16BE.decode(bytes).0.into_owned());
    }
    if let Some(encoding) = charset.and_then(|value| {
        encoding_rs::Encoding::for_label(value.trim_matches(['\'', '"']).as_bytes())
    }) {
        let (text, _, errors) = encoding.decode(bytes);
        if errors {
            return Err("文档编码无法完整解析".into());
        }
        return Ok(text.into_owned());
    }
    String::from_utf8(bytes.to_vec()).or_else(|_| {
        let (text, _, errors) = encoding_rs::GB18030.decode(bytes);
        if errors {
            Err("文档不是有效的 UTF-8 或 GB18030 文本".into())
        } else {
            Ok(text.into_owned())
        }
    })
}

pub fn resolve(base: &str, value: &str) -> Option<String> {
    let url = Url::parse(base).ok()?.join(value.trim()).ok()?;
    matches!(url.scheme(), "http" | "https" | "file").then(|| url.to_string())
}

pub fn parse(
    format: &str,
    text: &str,
    base: &str,
    fallback_title: &str,
) -> Result<(String, Vec<PageElement>, Vec<String>), String> {
    if format == "txt" {
        let elements = text
            .split("\n\n")
            .filter(|part| !part.trim().is_empty())
            .map(|part| PageElement::Paragraph {
                runs: vec![plain(part.trim())],
            })
            .collect();
        return Ok((fallback_title.into(), elements, vec![]));
    }
    let html = if matches!(format, "md" | "markdown") {
        let parser = pulldown_cmark::Parser::new_ext(
            text,
            pulldown_cmark::Options::ENABLE_TABLES
                | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
                | pulldown_cmark::Options::ENABLE_TASKLISTS,
        );
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, parser);
        html
    } else if matches!(format, "html" | "htm") {
        text.into()
    } else {
        return Err("仅支持 Markdown、TXT、HTML".into());
    };
    let document = Html::parse_document(&html);
    let root = document
        .select(&selector("article,main,[role=main]"))
        .next()
        .or_else(|| document.select(&selector("body")).next())
        .ok_or("HTML 缺少正文")?;
    let effective_base = document
        .select(&selector("base[href]"))
        .next()
        .and_then(|node| node.value().attr("href"))
        .and_then(|value| resolve(base, value))
        .unwrap_or_else(|| base.into());
    let title = root
        .select(&selector("h1"))
        .next()
        .or_else(|| document.select(&selector("title")).next())
        .map(|node| node.text().collect::<String>())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback_title.into());
    let mut elements = Vec::new();
    for node in root.select(&selector(
        "h1,h2,h3,h4,h5,h6,p,pre,li,table,img,video,iframe,hr",
    )) {
        if node
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|ancestor| {
                matches!(
                    ancestor.value().name(),
                    "script" | "style" | "nav" | "aside" | "header" | "footer" | "form"
                )
            })
        {
            continue;
        }
        let tag = node.value().name();
        if tag != "img"
            && node
                .ancestors()
                .skip(1)
                .filter_map(ElementRef::wrap)
                .any(|ancestor| matches!(ancestor.value().name(), "table" | "pre" | "li"))
        {
            continue;
        }
        match tag {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => elements.push(PageElement::Heading {
                level: tag[1..].parse().unwrap_or(2),
                text: node.text().collect::<String>(),
                anchor: node
                    .value()
                    .attr("id")
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("section-{}", elements.len())),
            }),
            "p" => {
                let runs = runs(node, &effective_base);
                if !runs.is_empty() {
                    elements.push(PageElement::Paragraph { runs });
                }
            }
            "li" => elements.push(PageElement::List {
                kind: if node
                    .parent()
                    .and_then(ElementRef::wrap)
                    .is_some_and(|parent| parent.value().name() == "ol")
                {
                    ListKind::Ordered
                } else {
                    ListKind::Bullet
                },
                indent: 1,
                items: vec![runs(node, &effective_base)],
            }),
            "pre" => elements.push(PageElement::Code {
                language: node
                    .select(&selector("code"))
                    .next()
                    .and_then(|code| code.value().attr("class"))
                    .unwrap_or_default()
                    .strip_prefix("language-")
                    .unwrap_or_default()
                    .into(),
                text: node.text().collect::<String>(),
            }),
            "table" => {
                let rows = node
                    .select(&selector("tr"))
                    .map(|row| {
                        row.select(&selector("th,td"))
                            .map(|cell| PageCell {
                                runs: runs(cell, &effective_base),
                                images: vec![],
                                header: cell.value().name() == "th",
                                colspan: cell
                                    .value()
                                    .attr("colspan")
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(1),
                                rowspan: cell
                                    .value()
                                    .attr("rowspan")
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(1),
                            })
                            .collect::<Vec<_>>()
                    })
                    .filter(|row| !row.is_empty())
                    .collect::<Vec<_>>();
                if !rows.is_empty() {
                    elements.push(PageElement::Table {
                        columns: vec![],
                        rows,
                    });
                }
            }
            "img" => {
                if let Some(src) = node
                    .value()
                    .attr("src")
                    .or_else(|| node.value().attr("data-src"))
                    .and_then(|value| resolve(&effective_base, value))
                {
                    elements.push(PageElement::Image { src, width: None });
                }
            }
            "video" | "iframe" => {
                if let Some(url) = node
                    .value()
                    .attr("src")
                    .and_then(|value| resolve(&effective_base, value))
                    .filter(|url| url.starts_with("http"))
                {
                    elements.push(PageElement::VideoLink { url });
                }
            }
            "hr" => elements.push(PageElement::Divider),
            _ => {}
        }
    }
    if !elements
        .iter()
        .any(|element| !element_text(element).trim().is_empty())
    {
        let text = runs(root, &effective_base)
            .into_iter()
            .map(|run| run.text)
            .collect::<Vec<_>>()
            .join(" ");
        if text.trim().is_empty() {
            return Err("未提取到正文，请检查网页范围或启用动态渲染".into());
        }
        elements.push(PageElement::Paragraph {
            runs: vec![plain(text.trim())],
        });
    }
    let links = root
        .select(&selector("a[href]"))
        .filter_map(|node| node.value().attr("href"))
        .filter_map(|value| resolve(&effective_base, value))
        .collect();
    Ok((title.trim().into(), elements, links))
}

fn plain(text: impl Into<String>) -> PageRun {
    PageRun {
        text: text.into(),
        bold: false,
        italic: false,
        color: None,
        link: None,
    }
}

fn runs(root: ElementRef<'_>, base: &str) -> Vec<PageRun> {
    fn walk(
        node: ElementRef<'_>,
        base: &str,
        bold: bool,
        italic: bool,
        link: Option<String>,
        output: &mut Vec<PageRun>,
    ) {
        let tag = node.value().name();
        if matches!(
            tag,
            "script"
                | "style"
                | "img"
                | "table"
                | "nav"
                | "aside"
                | "header"
                | "footer"
                | "form"
                | "noscript"
        ) {
            return;
        }
        let bold = bold || matches!(tag, "strong" | "b");
        let italic = italic || matches!(tag, "em" | "i");
        let link = if tag == "a" {
            node.value()
                .attr("href")
                .and_then(|value| resolve(base, value))
        } else {
            link
        };
        for child in node.children() {
            if let Some(text) = child.value().as_text() {
                if !text.trim().is_empty() {
                    output.push(PageRun {
                        text: text.to_string(),
                        bold,
                        italic,
                        color: None,
                        link: link.clone(),
                    });
                }
            } else if let Some(element) = ElementRef::wrap(child) {
                if element.value().name() == "br" {
                    output.push(plain("\n"));
                } else {
                    walk(element, base, bold, italic, link.clone(), output);
                }
            }
        }
    }
    let mut output = Vec::new();
    walk(root, base, false, false, None, &mut output);
    output
}

pub fn element_text(element: &PageElement) -> String {
    let joined = |runs: &[PageRun]| runs.iter().map(|run| run.text.as_str()).collect::<String>();
    match element {
        PageElement::Heading { text, .. } | PageElement::Code { text, .. } => text.clone(),
        PageElement::Paragraph { runs } => joined(runs),
        PageElement::List { items, .. } => items
            .iter()
            .map(|runs| joined(runs))
            .collect::<Vec<_>>()
            .join("\n"),
        PageElement::Table { rows, .. } => rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| joined(&cell.runs))
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Keep transfer and rendering blocks bounded without dropping text or running embedded HTML.
pub fn bounded(elements: Vec<PageElement>) -> Result<Vec<PageElement>, String> {
    fn parts(text: &str) -> Vec<String> {
        let positions = text
            .char_indices()
            .map(|(offset, _)| offset)
            .chain([text.len()])
            .collect::<Vec<_>>();
        (0..positions.len().saturating_sub(1))
            .step_by(8192)
            .map(|start| {
                text[positions[start]..positions[(start + 8192).min(positions.len() - 1)]]
                    .to_owned()
            })
            .collect()
    }
    let mut result = Vec::new();
    for element in elements {
        match element {
            PageElement::Paragraph { runs } => {
                let mut current = Vec::new();
                let mut size = 0;
                for run in runs {
                    for text in parts(&run.text) {
                        if size + text.len() > 32768 && !current.is_empty() {
                            result.push(PageElement::Paragraph {
                                runs: std::mem::take(&mut current),
                            });
                            size = 0;
                        }
                        size += text.len();
                        let mut part = run.clone();
                        part.text = text;
                        current.push(part);
                    }
                }
                if !current.is_empty() {
                    result.push(PageElement::Paragraph { runs: current });
                }
            }
            PageElement::Code { language, text } => {
                for text in parts(&text) {
                    result.push(PageElement::Code {
                        language: language.clone(),
                        text,
                    });
                }
            }
            PageElement::Table { columns, rows }
                if rows.len() > 64 && rows.iter().flatten().all(|cell| cell.rowspan == 1) =>
            {
                let header = rows
                    .first()
                    .filter(|row| row.iter().any(|cell| cell.header))
                    .cloned();
                for (index, rows) in rows.chunks(64).enumerate() {
                    let mut rows = rows.to_vec();
                    if index > 0
                        && let Some(header) = &header
                    {
                        rows.insert(0, header.clone());
                    }
                    result.push(PageElement::Table {
                        columns: columns.clone(),
                        rows,
                    });
                }
            }
            element => result.push(element),
        }
    }
    for element in &result {
        if serde_json::to_vec(element)
            .map_err(|error| error.to_string())?
            .len()
            > 1024 * 1024
        {
            return Err("单个结构化内容块超过 1 MiB，请拆分该资料".into());
        }
    }
    Ok(result)
}
