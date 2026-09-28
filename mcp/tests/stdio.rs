//! 端到端：真的拉起 `mcp-knowledge`，真的按 MCP 协议问一遍。
//!
//! 全程**离线**：先用自造夹具预置 catalog 与页面镜像，再让二进制只读本地缓存
//! （不联网、不渲染、不写盘）。夹具同样是自造的——官方正文不得进入仓库（调研 §9）。

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use wonderland_knowledge::cache::{Bundle, KnowledgeCache};
use wonderland_knowledge::mirror::{MirrorStore, PageMeta};
use wonderland_knowledge::page::{PageCell, PageElement, PageRun};
use wonderland_knowledge::{MirroredPage, PAGE_SCHEMA_VERSION};

const CATALOG: &str = r#"[
  { "title": "节点介绍", "path_id": "d_nodes", "children": [
      { "title": "服务器节点", "path_id": "d_server", "children": [
          { "title": "执行节点", "path_id": "d_exec", "children": [] }
      ]},
      { "title": "客户端节点", "path_id": "d_client", "children": [
          { "title": "运算节点", "path_id": "d_calc", "children": [] }
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

fn page(path_id: &str, elements: Vec<PageElement>) -> MirroredPage {
    MirroredPage {
        schema_version: PAGE_SCHEMA_VERSION,
        path_id: path_id.to_owned(),
        title: String::new(),
        url: String::new(),
        updated_at: None,
        mirrored_at: 0,
        elements,
    }
}

fn pages() -> Vec<MirroredPage> {
    vec![
        page(
            "d_exec",
            vec![
                heading(1, "一、通用", "a_gen"),
                heading(2, "建立字典", "a_build"),
                para("按配置ID建立字典并写入目标变量，可指定字典类型与默认值"),
                para("节点参数"),
                table(&[
                    &["参数名", "类型", "说明"],
                    &["字典ID", "字符串", "要建立的字典"],
                ]),
            ],
        ),
        page(
            "d_calc",
            vec![
                heading(1, "二、字典", "a_calc"),
                heading(2, "字典加速", "a_speed"),
                para("字典加速把字典按键排序以加速查询"),
                para("字典加速在批量查询时收益明显"),
                para("字典加速需要额外内存"),
            ],
        ),
        page(
            "d_unit",
            vec![
                heading(1, "一、单位状态", "a_unit"),
                para("单位状态表示实体当前所处的状态，可叠加多个效果"),
            ],
        ),
    ]
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wonderland-mcp-knowledge-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// 预置 catalog 缓存。
fn store_catalog(dir: &Path) {
    KnowledgeCache::new(dir.to_path_buf())
        .store(&Bundle::new("zh-cn", CATALOG.into()))
        .expect("预置 catalog");
}

/// 预置页面镜像。
fn store_pages(dir: &Path) {
    let store = MirrorStore::new(dir.to_path_buf());
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

/// 拉起二进制、发一条 initialize，返回（stdin, stdout, 子进程）。
fn spawn(dir: &Path) -> (impl Write, impl BufRead, std::process::Child) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-knowledge"))
        .arg("--cache-dir")
        .arg(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // stdout 才是协议通道；stderr 只作诊断
        .spawn()
        .expect("启动二进制");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let init = ask(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "probe", "version": "1" } }
        }),
    );
    assert_eq!(init["result"]["protocolVersion"], json!("2025-06-18"));
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(
        init["result"]["instructions"]
            .as_str()
            .is_some_and(|value| value.contains("千星奇域"))
    );
    (stdin, stdout, child)
}

#[test]
fn serves_the_six_tools_over_stdio() {
    let dir = temp_dir("stdio");
    store_catalog(&dir);
    store_pages(&dir);

    let (mut stdin, mut stdout, mut child) = spawn(&dir);

    // 通知不该产生回复：下一个响应的 id 必须是 2（若服务端回了通知，这里会读到 id 缺失的那条）
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .expect("写通知");
    stdin.flush().expect("flush");

    let listed = ask(
        &mut stdin,
        &mut stdout,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    );
    assert_eq!(listed["id"], json!(2), "通知不应产生回复");
    let tools = listed["result"]["tools"].as_array().expect("tools 数组");
    assert_eq!(tools.len(), 6);
    assert!(
        tools
            .iter()
            .all(|tool| tool["annotations"]["readOnlyHint"] == json!(true))
    );

    let node = ask(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "get_node", "arguments": { "names": ["建立字典"] } }
        }),
    );
    let text = node["result"]["content"][0]["text"]
        .as_str()
        .expect("文本内容");
    assert!(text.contains("a_build"), "应命中执行节点里的锚点：{text}");
    assert!(text.contains("server"), "应带出归属端：{text}");
    assert!(text.contains("字典ID"), "参数表要随节点带出来：{text}");
    assert_eq!(
        node["result"]["structuredContent"]["results"][0]["query"],
        json!("建立字典"),
        "声明 outputSchema 时应返回同形的 structuredContent"
    );

    let search = ask(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "search", "arguments": { "queries": ["字典加速有什么用"], "top_k": 3 } }
        }),
    );
    let hits = search["result"]["content"][0]["text"]
        .as_str()
        .expect("文本内容");
    assert!(
        hits.contains("运算节点"),
        "别名表应把该文档带进候选：{hits}"
    );

    let failed = ask(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "read_doc", "arguments": { "key": "不存在的文档" } }
        }),
    );
    assert_eq!(
        failed["result"]["isError"],
        json!(true),
        "工具错误要留在结果里"
    );

    drop(stdin); // 关闭 stdin → 服务端退出
    let status = child.wait().expect("等待退出");
    assert!(status.success(), "关闭 stdin 后应正常退出：{status}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_mirror_is_reported_as_a_tool_error() {
    let dir = temp_dir("no-mirror");
    store_catalog(&dir); // 只有目录，没镜像

    let (mut stdin, mut stdout, mut child) = spawn(&dir);

    let called = ask(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "search", "arguments": { "queries": ["字典"] } }
        }),
    );
    assert_eq!(
        called["result"]["isError"],
        json!(true),
        "缺镜像时工具调用要回 isError"
    );
    let text = called["result"]["content"][0]["text"]
        .as_str()
        .expect("文本内容");
    assert!(text.contains("镜像"), "原因要可读：{text}");

    drop(stdin);
    let status = child.wait().expect("等待退出");
    assert!(status.success(), "缺镜像不该让进程崩掉：{status}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 发一条请求、读回一条响应（换行分隔的 JSON-RPC）。
fn ask(stdin: &mut impl Write, stdout: &mut impl BufRead, request: Value) -> Value {
    writeln!(stdin, "{request}").expect("写请求");
    stdin.flush().expect("flush");
    let mut line = String::new();
    let read = stdout.read_line(&mut line).expect("读响应");
    assert!(read > 0, "服务端提前退出");
    serde_json::from_str(&line).expect("响应应是单行 JSON")
}
