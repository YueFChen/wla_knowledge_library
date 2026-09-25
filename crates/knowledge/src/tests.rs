//! 单元测试：全部使用**自造夹具**，不含任何官方正文（合规：官方内容不得进仓库，调研 §9）。
//!
//! 夹具 = 自造 `catalog.json` + 自造页面镜像，覆盖：按元素顺序切块、表格文本进检索语料、
//! 节点参数表来自镜像、重名文档 / 重名节点、目录页、空节点、端点分区、未镜像篇目不进索引。

use super::*;

/// 自造的目录树：覆盖重名文档、重名节点、目录页与端点分区。
const CATALOG: &str = r#"[
  { "title": "更新日志", "path_id": "d_update", "updated_at": "2026-08-01 10:00:00", "doc_type": "document", "children": [] },
  { "title": "节点介绍", "path_id": "d_nodes", "children": [
      { "title": "服务器节点", "path_id": "d_server", "children": [
          { "title": "执行节点", "path_id": "d_exec", "children": [] },
          { "title": "运算节点", "path_id": "d_calc_s", "children": [] }
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

fn exec_page() -> MirroredPage {
    page(
        "d_exec",
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
    )
}

fn calc_s_page() -> MirroredPage {
    page(
        "d_calc_s",
        vec![
            heading(1, "三、字典", "a_cs1"),
            heading(2, "建立字典", "a_build_s"),
            para("按配置ID建立字典并写入目标变量，可指定字典类型与默认值"),
        ],
    )
}

fn calc_c_page() -> MirroredPage {
    page(
        "d_calc_c",
        vec![
            heading(1, "五、字典", "a_cc1"),
            heading(2, "建立字典", "a_build_c"),
            para("按配置ID建立字典并写入目标变量，可指定字典类型与默认值"),
            heading(2, "字典加速", "a_speed_c"),
            para("字典加速把字典按键排序以加速查询"),
            para("字典加速在批量查询时收益明显"),
            para("字典加速需要额外内存"),
        ],
    )
}

fn unit_page() -> MirroredPage {
    page(
        "d_unit",
        vec![
            heading(1, "一、单位状态", "a_unit1"),
            para("单位状态表示实体当前所处的状态，可叠加多个效果"),
        ],
    )
}

fn update_page() -> MirroredPage {
    page(
        "d_update",
        vec![
            heading(1, "7.0版本-2026/08/12", "a_up1"),
            heading(2, "一、内容新增", "a_up2"),
            para("新增了字典加速相关说明"),
        ],
    )
}

fn all_pages() -> Vec<MirroredPage> {
    vec![
        exec_page(),
        calc_s_page(),
        calc_c_page(),
        unit_page(),
        update_page(),
    ]
}

fn setup() -> KnowledgeBase {
    build_from_json(CATALOG, &all_pages()).expect("夹具应能重建")
}

#[test]
fn rebuild_matches_documented_shape() {
    let kb = setup();
    let stats = kb.stats();
    assert_eq!(
        stats.docs, 9,
        "目录页也算节点：更新日志 / 节点介绍 / 服务器节点 / 执行节点 / 运算节点×2 / 客户端节点 / 概念介绍 / 单位状态"
    );
    assert_eq!(stats.leaf_docs, 5);
    assert_eq!(stats.mirrored_docs, 5, "五篇叶子都有镜像");
    assert_eq!(
        stats.blocks, 12,
        "按元素顺序切块：段落 / 列表项各一块，表格一块"
    );
    assert_eq!(stats.tables, 1, "只有执行节点里那张参数表");
    assert_eq!(stats.nodes, 5, "「空节点」内容太少，不建条目");
    assert_eq!(stats.nodes_with_params, 1, "只有打印字符串有参数表");
    assert_eq!(stats.nodes_server, 3);
    assert_eq!(stats.nodes_client, 2);
    assert!(stats.chars > 0);
    assert_eq!(stats.schema_version, SCHEMA_VERSION);
}

#[test]
fn blocks_follow_element_order_and_carry_breadcrumb() {
    let kb = setup();
    let doc = kb.read_doc("d_exec", None).expect("执行节点已镜像");
    assert_eq!(doc.title, "执行节点");
    assert!(doc.has_tables, "该篇含结构化表格");

    // 锚点顺序 = 镜像里的元素顺序（标题只改层级路径，不单独成块）。
    let anchors: Vec<&str> = doc.blocks.iter().map(|b| b.anchor.as_str()).collect();
    assert_eq!(
        anchors,
        vec!["a_print", "a_print", "a_print", "a_empty", "a_build_exec"]
    );
    assert_eq!(doc.blocks[0].text, "节点功能满足条件时输出文本");

    // 块的面包屑 = 文档面包屑 + 标题层级路径（序号前缀被剥掉）。
    let build = doc
        .blocks
        .iter()
        .find(|b| b.anchor == "a_build_exec")
        .expect("应有「建立字典」的块");
    assert_eq!(
        build.breadcrumb,
        vec!["节点介绍", "服务器节点", "执行节点", "二、字典", "建立字典"]
    );
}

#[test]
fn table_text_enters_the_retrieval_corpus() {
    let kb = setup();
    let hits = kb.search(&["参数名".to_owned()], 5);
    assert!(
        hits[0]
            .iter()
            .any(|hit| hit.text.contains("参数名") && hit.text.contains("要输出的内容")),
        "表格单元格内容要进检索文本（按行拼接）：{:?}",
        hits[0].iter().map(|hit| &hit.text).collect::<Vec<_>>()
    );
    // 结构化表格留在块上（检索文本是压平的，列边界不能丢）。
    let doc = kb.read_doc("d_exec", None).expect("执行节点已镜像");
    assert!(
        kb.search(&["要输出的内容".to_owned()], 1)[0]
            .iter()
            .any(|hit| hit.anchor == "a_print"),
        "表格所在标题的锚点要能命中"
    );
    // 表格块按行拼接：行内以空格分隔、行间是换行。
    assert!(
        doc.blocks
            .iter()
            .any(|b| b.text.contains("参数名 类型 说明") && b.text.contains('\n')),
        "表格块要保留行结构：{:?}",
        doc.blocks.iter().map(|b| &b.text).collect::<Vec<_>>()
    );
}

#[test]
fn node_params_come_from_the_mirror_table() {
    let kb = setup();
    let lookup = kb.get_node(&["打印字符串".to_owned()]);
    assert_eq!(lookup.len(), 1);
    assert_eq!(lookup[0].matches.len(), 1);
    let node = &lookup[0].matches[0];
    assert_eq!(node.anchor, "a_print");
    assert_eq!(node.side, Some(Side::Server));
    assert_eq!(
        node.breadcrumb,
        vec![
            "节点介绍",
            "服务器节点",
            "执行节点",
            "一、通用",
            "打印字符串"
        ]
    );
    assert_eq!(
        node.params,
        vec![
            vec!["参数名", "类型", "说明"],
            vec!["文本", "字符串", "要输出的内容"]
        ],
        "参数表要带上表头行"
    );
    assert!(!node.params_need_render);

    // 镜像里没有表格的节点：params 为空，标记为需要回原文。
    let build = kb.get_node(&["建立字典".to_owned()]);
    assert_eq!(build[0].matches.len(), 3);
    assert!(
        build[0]
            .matches
            .iter()
            .all(|m| m.params.is_empty() && m.params_need_render)
    );
}

#[test]
fn unmirrored_leaf_docs_stay_out_of_the_index() {
    // 只镜像执行节点：其余篇目没有块，不进检索，`read_doc` 也拿不到。
    let kb = build_from_json(CATALOG, &[exec_page()]).expect("夹具应能重建");
    let stats = kb.stats();
    assert_eq!(stats.mirrored_docs, 1);
    assert!(kb.read_doc("d_unit", None).is_none(), "未镜像的篇目读不到");
    assert!(kb.read_doc("单位状态", None).is_none());
    assert!(
        kb.search(&["单位状态表示实体".to_owned()], 5)[0]
            .iter()
            .all(|hit| hit.path_id != "d_unit"),
        "未镜像的篇目不进检索"
    );
    assert!(kb.read_doc("d_exec", None).is_some());
    assert!(
        kb.read_doc("d_nodes", None).is_none(),
        "未镜像的目录节点也不能伪装成一篇空文档"
    );
    // 文档树仍完整：镜像作业要按它列出待镜像篇目。
    assert_eq!(stats.docs, 9);
    assert_eq!(stats.leaf_docs, 5);
}

#[test]
fn clean_heading_keeps_version_like_titles() {
    assert_eq!(clean_heading("1. 打印字符串"), "打印字符串");
    assert_eq!(clean_heading("**11.切换自身执行状态**"), "切换自身执行状态");
    assert_eq!(clean_heading("（1）概述"), "概述");
    assert_eq!(clean_heading("7.0版本-2026/08/12"), "7.0版本-2026/08/12");
    assert_eq!(clean_heading("二、字典"), "二、字典");
}

#[test]
fn get_node_returns_all_same_named_candidates() {
    let kb = setup();
    let lookup = kb.get_node(&["建立字典".to_owned()]);
    assert_eq!(lookup.len(), 1);
    assert_eq!(
        lookup[0].matches.len(),
        3,
        "同名节点分布在三篇文档里，必须整组返回而不是猜一个（调研 §13.11）"
    );
    let sides: Vec<Option<Side>> = lookup[0].matches.iter().map(|m| m.side).collect();
    assert!(sides.contains(&Some(Side::Server)) && sides.contains(&Some(Side::Client)));
}

#[test]
fn get_node_narrowed_uses_side_hint() {
    let kb = setup();
    let narrowed = kb.get_node_narrowed(
        &["建立字典".to_owned()],
        "服务端的「建立字典」节点有哪些参数？",
    );
    assert!(!narrowed[0].matches.is_empty());
    assert!(
        narrowed[0]
            .matches
            .iter()
            .all(|m| m.side == Some(Side::Server))
    );

    let narrowed = kb.get_node_narrowed(
        &["建立字典".to_owned()],
        "客户端节点里的「建立字典」节点有哪些参数？",
    );
    assert!(
        narrowed[0]
            .matches
            .iter()
            .all(|m| m.side == Some(Side::Client))
    );
}

#[test]
fn search_converges_on_the_named_node() {
    let kb = setup();
    let hits = kb.search(&["「建立字典」节点有哪些参数？".to_owned()], 5);
    let top = &hits[0][0];
    assert_eq!(top.kind, HitKind::CandidateNode);
    assert!(
        ["a_build_exec", "a_build_s", "a_build_c"].contains(&top.anchor.as_str()),
        "首位应命中「建立字典」的候选块，实际 {}",
        top.anchor
    );
    assert!(top.text.contains("建立字典"));
}

#[test]
fn alias_table_bridges_wording_gap() {
    let kb = setup();
    // 「字典加速」只在「运算节点」正文里重复出现，别名表应把它和该文档关联起来
    let hits = kb.search(&["字典加速适合什么场景".to_owned()], 5);
    assert!(
        hits[0].iter().any(|hit| hit.doc_title == "运算节点"),
        "别名表应把该文档带进候选，实际：{:?}",
        hits[0].iter().map(|h| &h.doc_title).collect::<Vec<_>>()
    );
}

#[test]
fn list_docs_and_read_doc() {
    let kb = setup();
    let groups = kb.list_docs(&["运算节点".to_owned()]);
    assert_eq!(groups[0].total, 2, "同名文档两篇都应列出");
    assert_eq!(kb.list_docs(&["绝对不存在的标题".to_owned()])[0].total, 0);
    assert_eq!(kb.list_docs(&[])[0].total, 9, "关键词留空即列全部");

    let doc = kb.read_doc("d_exec", None).expect("按 path_id 取文档");
    assert_eq!(doc.title, "执行节点");
    assert!(doc.blocks.len() >= 2);

    let from_anchor = kb
        .read_doc("d_exec", Some("a_build_exec"))
        .expect("按锚点起裁");
    assert_eq!(
        from_anchor.blocks.first().map(|b| b.anchor.as_str()),
        Some("a_build_exec")
    );
}

#[test]
fn detail_url_points_at_official_page() {
    assert_eq!(
        render::detail_url("mh277t9fl4tm"),
        "https://act.mihoyo.com/ys/ugc/tutorial/detail/mh277t9fl4tm"
    );
}
