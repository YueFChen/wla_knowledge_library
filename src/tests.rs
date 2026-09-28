//! 单元测试：夹具自造，全程离线（官方正文不得进仓库，调研 §9）。
//!
//! 覆盖两件事：入口的**状态口径**（有没有 catalog、有没有镜像、索引规模、新鲜度）与
//! **四组查询的包装**（字段是否与 MCP 侧同一套取值）。检索质量本身由
//! 插件内部 `wonderland-knowledge` crate 的测试负责，这里不重复。
//!
//! 内容源是页面镜像，因此"预置一份缓存"= 存 catalog + 写页面镜像。

use std::path::Path;

use wonderland_knowledge::cache::Bundle;
use wonderland_knowledge::mirror::PageMeta;

use super::*;

/// 自造的目录树：重名文档、重名节点、目录页与端点分区都覆盖到。
const CATALOG: &str = r#"[
  { "title": "更新日志", "path_id": "d_update", "updated_at": "2026-08-01 10:00:00", "doc_type": "document", "children": [] },
  { "title": "节点介绍", "path_id": "d_nodes", "children": [
      { "title": "服务器节点", "path_id": "d_server", "children": [
          { "title": "执行节点", "path_id": "d_exec", "children": [] }
      ]},
      { "title": "客户端节点", "path_id": "d_client", "children": [
          { "title": "运算节点", "path_id": "d_calc_c", "children": [] }
      ]}
  ]},
  { "title": "概念介绍", "path_id": "d_concept", "children": [
      { "title": "单位状态", "path_id": "d_unit", "children": [] }
  ]}
]"#;

fn run(text: &str) -> PageRun {
    PageRun {
        text: text.to_owned(),
        bold: false,
        italic: false,
        color: None,
        link: None,
    }
}

fn heading(level: u8, text: &str, anchor: &str) -> PageElement {
    PageElement::Heading {
        level,
        text: text.to_owned(),
        anchor: anchor.to_owned(),
    }
}

fn para(text: &str) -> PageElement {
    PageElement::Paragraph {
        runs: vec![run(text)],
    }
}

fn table(rows: &[&[&str]]) -> PageElement {
    PageElement::Table {
        columns: Vec::new(),
        rows: rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| PageCell {
                        runs: vec![run(cell)],
                        images: Vec::new(),
                        colspan: 1,
                        rowspan: 1,
                        header: false,
                    })
                    .collect()
            })
            .collect(),
    }
}

fn page(path_id: &str, updated_at: Option<&str>, elements: Vec<PageElement>) -> MirroredPage {
    MirroredPage {
        schema_version: wonderland_knowledge::PAGE_SCHEMA_VERSION,
        path_id: path_id.to_owned(),
        title: String::new(),
        url: String::new(),
        updated_at: updated_at.map(str::to_owned),
        mirrored_at: 0,
        elements,
    }
}

fn pages() -> Vec<MirroredPage> {
    vec![
        page(
            "d_exec",
            None,
            vec![
                heading(1, "一、通用", "a_ex1"),
                heading(2, "1. 打印字符串", "a_print"),
                para("节点功能满足条件时输出文本"),
                para("节点参数"),
                table(&[
                    &["参数名", "类型", "说明"],
                    &["文本", "字符串", "要输出的内容"],
                ]),
                heading(2, "2. 空节点", "a_empty"),
                para("待补"),
                heading(1, "二、字典", "a_ex2"),
                heading(2, "3. 建立字典", "a_build_exec"),
                para("按配置ID建立字典并写入目标变量，可指定字典类型与默认值"),
            ],
        ),
        page(
            "d_calc_c",
            None,
            vec![
                heading(1, "五、字典", "a_cc1"),
                heading(2, "建立字典", "a_build_c"),
                para("按配置ID建立字典并写入目标变量，可指定字典类型与默认值"),
                heading(2, "字典加速", "a_speed_c"),
                para("字典加速把字典按键排序以加速查询"),
                para("字典加速在批量查询时收益明显"),
                para("字典加速需要额外内存"),
            ],
        ),
        page(
            "d_unit",
            None,
            vec![
                heading(1, "一、单位状态", "a_unit1"),
                para("单位状态表示实体当前所处的状态，可叠加多个效果"),
            ],
        ),
        page(
            "d_update",
            Some("2026-08-01 10:00:00"),
            vec![
                heading(1, "7.0版本-2026/08/12", "a_up1"),
                heading(2, "一、内容新增", "a_up2"),
                para("新增了字典加速相关说明"),
            ],
        ),
    ]
}

/// 建一个装了临时应用数据目录的插件实例；每个用例用各自的 tag，避免互相踩缓存。
fn library(tag: &str) -> KnowledgeLibrary {
    let app_data_dir = std::env::temp_dir().join(format!(
        "wonderland-knowledge-library-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&app_data_dir);
    KnowledgeLibrary::new(app_data_dir.join("plugin-data/knowledge_library"))
}

/// 只预置 catalog（还没镜像）。
fn store_catalog(library: &KnowledgeLibrary) {
    library
        .cache
        .store(&Bundle::new(DEFAULT_LANG, CATALOG.to_owned()))
        .expect("预置 catalog 缓存");
}

/// 预置 catalog + 全部页面镜像（不联网）。
fn store(library: &KnowledgeLibrary) {
    store_catalog(library);
    let store = library.store();
    let mut index = store.load_index();
    for page in pages() {
        store.save_page(&page).expect("写页面镜像");
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
    }
    store.save_index(&index).expect("写镜像索引");
}

#[test]
fn status_before_the_first_fetch_points_at_the_fetch_action() {
    let library = library("no-cache");
    let status = library.status();
    assert!(!status.ready);
    assert_eq!(status.docs, 0);
    assert!(
        status
            .error
            .as_deref()
            .is_some_and(|error| error.contains("获取目录")),
        "没有缓存时要指向获取目录动作：{:?}",
        status.error
    );
    assert!(
        Path::new(&status.cache_dir).ends_with(Path::new("plugin-data/knowledge_library")),
        "缓存目录要与 MCP 二进制算出的同一份：{}",
        status.cache_dir
    );
    assert!(status.cached_at.is_none() && status.age_days.is_none());
    assert!(!status.stale);
}

#[test]
fn status_with_catalog_but_no_mirror_points_at_mirroring() {
    let library = library("no-mirror");
    store_catalog(&library);
    let status = library.status();
    assert!(!status.ready, "还没有镜像，谈不上可用");
    assert!(
        status
            .error
            .as_deref()
            .is_some_and(|error| error.contains("镜像")),
        "有目录但没镜像时要指向镜像动作：{:?}",
        status.error
    );
    assert_eq!(status.docs, 8, "目录落盘后就应立即报告完整目录规模");
    assert!(status.cached_at.is_some(), "catalog 已经在了");
    assert_eq!(
        library.list_docs(vec![]).expect("无镜像也应能列目录")[0].total,
        8
    );
}

#[test]
fn status_after_mirror_reports_scale_and_freshness() {
    let library = library("cached");
    store(&library);
    let status = library.status();
    assert!(status.ready, "有目录 + 镜像就该可用：{:?}", status.error);
    assert!(status.error.is_none());
    assert_eq!(
        status.docs, 8,
        "目录页也算节点：更新日志 / 节点介绍 / 服务器节点 / 执行节点 / 客户端节点 / 运算节点 / 概念介绍 / 单位状态"
    );
    assert_eq!(status.leaf_docs, 4);
    assert_eq!(status.nodes, status.nodes_server + status.nodes_client);
    assert!(status.nodes_server >= 1 && status.nodes_client >= 1);
    assert!(status.blocks > 0 && status.chars > 0);
    assert!(status.table_blocks >= 1, "含表格的块要被计数");
    assert_eq!(status.schema_version, SCHEMA_VERSION);
    assert!(status.cached_at.is_some_and(|at| at > 0));
    assert!(status.age_days.is_some_and(|days| days < 1.0));
    assert!(!status.stale, "刚存下的缓存不该判为过期");
}

#[test]
fn queries_without_cache_explain_what_to_do() {
    let library = library("no-cache-query");
    let error = library
        .search(vec!["字典".to_owned()], 5)
        .expect_err("没有缓存时查询应失败")
        .to_string();
    assert!(error.contains("获取目录"), "应指向获取目录动作：{error}");
}

#[test]
fn queries_without_mirror_explain_what_to_do() {
    let library = library("no-mirror-query");
    store_catalog(&library);
    let error = library
        .search(vec!["字典".to_owned()], 5)
        .expect_err("没镜像时查询应失败")
        .to_string();
    assert!(error.contains("镜像"), "应指向镜像动作：{error}");
}

#[test]
fn node_lookup_keeps_breadcrumb_and_side() {
    let library = library("node");
    store(&library);
    let groups = library
        .get_node(vec!["建立字典".to_owned()])
        .expect("节点查询");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].query, "建立字典");
    let sides: Vec<Option<String>> = groups[0].matches.iter().map(|m| m.side.clone()).collect();
    assert!(
        sides.contains(&Some("server".to_owned())) && sides.contains(&Some("client".to_owned())),
        "端点要按 server / client 原样带出：{sides:?}"
    );
    let exec = groups[0]
        .matches
        .iter()
        .find(|m| m.anchor == "a_build_exec")
        .expect("执行节点里的候选应在");
    assert_eq!(exec.doc_title, "执行节点");
    assert_eq!(
        exec.breadcrumb,
        vec!["节点介绍", "服务器节点", "执行节点", "二、字典", "建立字典"]
    );
    assert!(
        exec.url.ends_with(&exec.path_id),
        "要能跳回原文：{}",
        exec.url
    );
}

#[test]
fn node_params_come_from_the_mirror_table() {
    let library = library("params");
    store(&library);
    let groups = library
        .get_node(vec!["打印字符串".to_owned()])
        .expect("节点查询");
    let node = &groups[0].matches[0];
    assert_eq!(
        node.params,
        vec![
            vec!["参数名", "类型", "说明"],
            vec!["文本", "字符串", "要输出的内容"]
        ],
        "参数表要随节点带出来（含表头行）"
    );
    assert!(!node.params_need_render, "参数表已从镜像取到");
}

#[test]
fn search_list_docs_and_read_doc_share_the_index() {
    let library = library("read");
    store(&library);

    let groups = library
        .search(vec!["字典加速适合什么场景".to_owned()], 5)
        .expect("检索");
    assert_eq!(groups[0].query, "字典加速适合什么场景");
    assert!(
        groups[0].hits.iter().any(|hit| hit.doc_title == "运算节点"),
        "别名表应把该文档带进候选：{:?}",
        groups[0]
            .hits
            .iter()
            .map(|hit| &hit.doc_title)
            .collect::<Vec<_>>()
    );
    let first = &groups[0].hits[0];
    assert!(
        ["candidate_node", "candidate_doc", "sparse"].contains(&first.kind.as_str()),
        "命中来源要与 MCP 侧同一套取值：{}",
        first.kind
    );

    let all = library.list_docs(vec![]).expect("列全部");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].total, 8, "关键词留空即列全部");
    let named = library
        .list_docs(vec!["运算节点".to_owned()])
        .expect("按名筛");
    assert_eq!(named[0].total, 1);
    assert_eq!(named[0].docs[0].breadcrumb[0], "节点介绍");

    let doc = library
        .read_doc("d_exec".to_owned(), None)
        .expect("按 path_id 读");
    assert_eq!(doc.title, "执行节点");
    assert!(doc.has_tables, "该篇含结构化表格");
    assert!(!doc.blocks.is_empty());
    let from_anchor = library
        .read_doc("执行节点".to_owned(), Some("a_build_exec".to_owned()))
        .expect("按标题 + 锚点读");
    assert_eq!(
        from_anchor
            .blocks
            .first()
            .map(|block| block.anchor.as_str()),
        Some("a_build_exec")
    );
}

#[test]
fn invalid_input_is_rejected_with_a_readable_reason() {
    let library = library("input");
    store(&library);
    assert!(
        library
            .search(vec![], 5)
            .expect_err("空查询应被拒")
            .to_string()
            .contains("queries")
    );
    assert!(
        library
            .search(vec!["  ".to_owned()], 5)
            .expect_err("空白项应被拒")
            .to_string()
            .contains("queries")
    );
    assert!(
        library
            .search(vec!["字典".to_owned()], 99)
            .expect_err("top_k 越界应被拒")
            .to_string()
            .contains("1–20")
    );
    assert!(
        library
            .get_node(vec![])
            .expect_err("空节点名应被拒")
            .to_string()
            .contains("names")
    );
    assert!(
        library
            .read_doc("   ".to_owned(), None)
            .expect_err("空 key 应被拒")
            .to_string()
            .contains("path_id")
    );
}

#[test]
fn unknown_document_key_is_reported_like_the_mcp_entry() {
    let library = library("unknown-doc");
    store(&library);
    let error = library
        .read_doc("不存在的文档".to_owned(), None)
        .expect_err("不存在的文档应报错")
        .to_string();
    assert!(error.contains("没有标题或 path_id 匹配"), "{error}");
}

#[test]
fn unmirrored_document_is_not_readable() {
    let library = library("unmirrored-read");
    store_catalog(&library);
    let error = library
        .read_doc("执行节点".to_owned(), None)
        .expect_err("没镜像时读不到正文")
        .to_string();
    assert!(error.contains("镜像"), "应提示先镜像：{error}");
}

#[test]
fn mirror_status_counts_targets_without_needing_mirrors() {
    let library = library("mirror-status");
    store_catalog(&library);
    let status = library
        .mirror_status()
        .expect("按 catalog 就能给出镜像状态");
    assert_eq!(status.mirrored, 0);
    assert_eq!(status.total, 8, "catalog 的目录节点也有正文，全部都要镜像");
    assert!(!status.running);

    store(&library);
    let status = library.mirror_status().expect("镜像状态");
    assert_eq!(status.mirrored, 4);
    assert_eq!(status.total, 8);
    assert_eq!(status.outdated, 0, "updated_at 没变，不用重镜像");
    assert!(status.elements > 0);
}

#[test]
fn mcp_config_uses_the_binary_path_or_keeps_a_template() {
    let cache_dir = Path::new(
        r"C:\Users\tester\AppData\Roaming\com.wonderland.assistant\plugin-data\knowledge_library",
    );
    let found = mcp_config(Some(Path::new(r"C:\tools\mcp-knowledge.exe")), cache_dir);
    let value: serde_json::Value = serde_json::from_str(&found).expect("片段应是合法 JSON");
    assert_eq!(
        value["mcpServers"]["wonderland-knowledge"]["command"],
        json!(r"C:\tools\mcp-knowledge.exe")
    );
    assert_eq!(
        value["mcpServers"]["wonderland-knowledge"]["args"],
        json!(["--cache-dir", cache_dir.display().to_string()])
    );
    assert!(value["_note"].is_null(), "找得到二进制就不附提示");

    let missing = mcp_config(None, cache_dir);
    let value: serde_json::Value = serde_json::from_str(&missing).expect("模板也应是合法 JSON");
    assert_eq!(
        value["mcpServers"]["wonderland-knowledge"]["command"],
        json!(MCP_BINARY_PLACEHOLDER)
    );
    assert!(
        value["_note"]
            .as_str()
            .is_some_and(|note| note.contains(MCP_BINARY)),
        "找不到二进制时要附上该改哪里的提示"
    );
}

/// `cache_dir()` 与状态里报的目录必须是同一个（宿主据此打开缓存目录）。
#[test]
fn cache_dir_is_where_the_plugin_says() {
    let library = library("cache-dir");
    assert_eq!(
        library.status().cache_dir,
        library.cache_dir().display().to_string()
    );
}

/// 原文链接只能来自目录：前端传的是 `path_id`，不是地址。
#[test]
fn source_url_only_resolves_known_path_ids() {
    let library = library("source-url");
    store(&library);
    let url = library
        .source_url("d_exec")
        .expect("已知 path_id 应给出原文链接");
    assert!(url.ends_with("d_exec"), "应是官方详情页链接：{url}");
    assert!(
        library.source_url("not-a-doc").is_err(),
        "未知 path_id 不能给出地址"
    );
}
