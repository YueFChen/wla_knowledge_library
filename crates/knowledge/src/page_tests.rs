//! 页面提取的单元测试：夹具按**实测的官方 DOM 写法自造**（类名、`data-list-*` 属性照抄结构，
//! 文字与地址全部自造，官方正文不进仓库，调研 §9）。

use crate::page::*;

/// 覆盖全部元素形态：标题、段落（加粗 / 斜体 / 着色 / 站内链接 / 换行）、表格（列宽 + 跨度）、
/// 列表（同级连续、两种类型）、图片（`src` / `data-src` / 不可信域）、分隔线，以及正文容器之外的页脚。
const DOM: &str = r#"<html><body>
<div class="tw-pt-9"><h1 class="tw-mb-9 tw-text-[38px]">页面大标题</h1></div>
<div class="doc-view tw-mb-9" style="min-height: 10px;">
  <h1 id="a1" data-block-mark="{}"><strong>一、通用</strong></h1>
  <h2 id="a2" data-block-mark="{}"><strong>1. 示例节点</strong></h2>
  <h3 id="a3">1.1 补充说明</h3>
  <p id="p1">可以在日志中输出一条<strong>字符串</strong>，用于调试<br>第二行</p>
  <p id="p2"><span style="color: rgba(245,34,45,1)"><em>注意</em></span>：该字段<em>必填</em></p>
  <p id="p3"><span><img style="width: 420px;aspect-ratio:1.7;" src="https://act-webstatic.mihoyo.com/ugc-tutorial/knowledge/cn/zh-cn/demo/aaa.png" class="lazy entered loaded"></span></p>
  <p id="p4"><img data-src="https://act-webstatic.mihoyo.com/ugc-tutorial/knowledge/cn/zh-cn/demo/bbb.png" class="lazy"></p>
  <p id="p5"><img src="https://evil.example.com/x.png"></p>
  <p id="p6"><strong>节点参数</strong></p>
  <div class="table-wrapper"><table id="t1" class="table-wrapper" border="1" cellspacing="0">
    <colgroup><col style="width: 245px;"><col style="width: 120px;"></colgroup>
    <tbody style="vertical-align: top">
      <tr class="table-row"><td id="c1" colspan="1" rowspan="1" data-colwidth="245"><p><strong>参数名</strong></p></td><td colspan="2" rowspan="1"><p><strong>类型</strong></p></td></tr>
      <tr class="table-row"><td colspan="1" rowspan="1"><p>文本</p></td><td colspan="1" rowspan="1"><p>字符串</p><p><img src="https://act-webstatic.mihoyo.com/ugc-tutorial/knowledge/cn/zh-cn/demo/ccc.png"></p></td></tr>
    </tbody>
  </table></div>
  <div class="prosemirror-list-new" data-list-kind="bullet" data-list-indent="1"><div class="list-marker list-marker-click-target">•</div><div class="list-content"><span>第一条</span></div></div>
  <div class="prosemirror-list-new" data-list-kind="bullet" data-list-indent="1"><div class="list-marker list-marker-click-target">•</div><div class="list-content"><span>第二条</span></div></div>
  <div class="prosemirror-list-new" data-list-kind="ordered" data-list-indent="1"><div class="list-marker">1</div><div class="list-content">有序项</div></div>
  <p id="p7">站内参考 <a href="/ys/ugc/tutorial/detail/other">另一篇</a></p>
  <div class="split-line tw-h-[1px]"></div>
</div>
<div class="footer">页脚不该进正文</div>
</body></html>"#;

fn elements() -> Vec<PageElement> {
    elements_from_dom(DOM)
}

fn headings(list: &[PageElement]) -> Vec<(u8, String, String)> {
    list.iter()
        .filter_map(|element| {
            element
                .heading()
                .map(|(level, text, anchor)| (level, text.to_owned(), anchor.to_owned()))
        })
        .collect()
}

fn paragraphs(list: &[PageElement]) -> Vec<Vec<PageRun>> {
    list.iter()
        .filter_map(|element| match element {
            PageElement::Paragraph { runs } => Some(runs.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn headings_keep_level_text_and_anchor() {
    assert_eq!(
        headings(&elements()),
        vec![
            (1, "一、通用".to_owned(), "a1".to_owned()),
            (2, "1. 示例节点".to_owned(), "a2".to_owned()),
            (3, "1.1 补充说明".to_owned(), "a3".to_owned()),
        ]
    );
}

#[test]
fn paragraphs_carry_inline_marks_and_line_breaks() {
    let list = paragraphs(&elements());
    let first = &list[0];
    let text: String = first.iter().map(|run| run.text.clone()).collect();
    assert!(text.contains("\n第二行"), "br 要保留成换行：{text:?}");
    let bold = first.iter().find(|run| run.bold).expect("加粗要带出来");
    assert_eq!(bold.text, "字符串");

    let second = &list[1];
    assert!(second[0].italic && second[0].color.as_deref() == Some("rgba(245,34,45,1)"));
    assert_eq!(second[0].text, "注意");
    assert_eq!(second[1].text, "：该字段");
    assert!(second[2].italic);
}

#[test]
fn in_page_links_are_made_absolute() {
    let list = paragraphs(&elements());
    let last = list.last().expect("末段含站内链接");
    let link = last
        .iter()
        .find(|run| run.link.is_some())
        .expect("应保留链接");
    assert_eq!(
        link.link.as_deref(),
        Some("https://act.mihoyo.com/ys/ugc/tutorial/detail/other")
    );
    assert_eq!(link.text, "另一篇");
}

#[test]
fn only_trusted_image_hosts_are_kept() {
    let images: Vec<(String, Option<u32>)> = elements()
        .into_iter()
        .filter_map(|element| match element {
            PageElement::Image { src, width } => Some((src, width)),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2, "不可信域名的图要丢掉");
    assert_eq!(
        images[0].0,
        "https://act-webstatic.mihoyo.com/ugc-tutorial/knowledge/cn/zh-cn/demo/aaa.png"
    );
    assert_eq!(images[0].1, Some(420));
    assert!(images[1].0.ends_with("bbb.png"), "没有 src 时用 data-src");
    assert_eq!(images[1].1, None);
}

#[test]
fn tables_keep_column_widths_spans_and_header_row() {
    let table = elements()
        .into_iter()
        .find_map(|element| match element {
            PageElement::Table { columns, rows } => Some((columns, rows)),
            _ => None,
        })
        .expect("应提取到表格");
    assert_eq!(table.0, vec![245, 120]);
    assert_eq!(table.1.len(), 2);
    assert!(table.1[0].iter().all(|cell| cell.header), "首行做表头");
    assert!(!table.1[1].iter().any(|cell| cell.header));
    assert_eq!(table.1[0][0].runs[0].text, "参数名");
    assert!(table.1[0][0].runs[0].bold);
    assert_eq!(table.1[0][1].colspan, 2, "跨列要保留");
    assert_eq!(table.1[1][1].runs[0].text, "字符串");
    assert_eq!(
        table.1[1][1].images,
        vec![
            "https://act-webstatic.mihoyo.com/ugc-tutorial/knowledge/cn/zh-cn/demo/ccc.png"
                .to_owned()
        ],
        "单元格里的示意图要单独收，不能降级成文本"
    );
}

#[test]
fn consecutive_list_nodes_merge_into_one_list_per_kind() {
    let lists: Vec<(ListKind, u8, Vec<String>)> = elements()
        .into_iter()
        .filter_map(|element| match element {
            PageElement::List {
                kind,
                indent,
                items,
            } => Some((
                kind,
                indent,
                items
                    .into_iter()
                    .map(|runs| runs.into_iter().map(|run| run.text).collect())
                    .collect(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(lists.len(), 2, "两项无序列表合成一个，有序列表另起一个");
    assert_eq!(lists[0].0, ListKind::Bullet);
    assert_eq!(lists[0].1, 1);
    assert_eq!(lists[0].2, vec!["第一条".to_owned(), "第二条".to_owned()]);
    assert_eq!(lists[1].0, ListKind::Ordered);
    assert_eq!(lists[1].2, vec!["有序项".to_owned()]);
}

#[test]
fn container_boundary_excludes_the_footer_and_keeps_the_divider() {
    let list = elements();
    assert!(list.contains(&PageElement::Divider));
    let text = list
        .iter()
        .filter_map(|element| match element {
            PageElement::Paragraph { runs } => {
                Some(runs.iter().map(|run| run.text.clone()).collect::<String>())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    assert!(!text.contains("页脚"), "容器之外的页脚不能混进来");
    assert!(!text.contains("页面大标题"), "正文外的页面大标题不算正文");
}

#[test]
fn missing_container_or_broken_input_yields_no_panic() {
    assert!(elements_from_dom("<div>没有正文容器</div>").is_empty());
    assert!(elements_from_dom("").is_empty());
    // 畸形输入：只要求不 panic，能拿到多少算多少。
    let _ = elements_from_dom("<div class=\"doc-view\"><p>未闭合<span>内层");
    let _ = elements_from_dom("<div class=doc-view><p>x</p></div>");
}
