//! 极小的 DOM 解析：只够把浏览器 `--dump-dom` 的正文拆成树。
//!
//! 为什么不引现成解析器：这里处理的是**浏览器序列化后的** HTML（标签闭合规范、属性统一双引号、
//! 自闭合明确），只需几百行；而 `render.rs` 已经确立了"不为单一用途引依赖"的做法。
//!
//! 语义按"够用"取舍：
//! - 只认开始 / 结束 / 自闭合三种记号，属性只取 `name="value"` 形式（浏览器输出就是这样）；
//! - `<script>` / `<style>` 整个跳过（正文里没有，但页面头部有，跳过省内存）；
//! - 结束标签与栈顶不匹配时向上找同名标签弹出（浏览器之外的输入也能容忍，不 panic）。

/// 一个元素节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Child>,
}

/// 子节点：元素或文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Child {
    Node(Node),
    Text(String),
}

impl Node {
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            attrs: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// `class` 里是否含某个类名（按空格切分，不接受子串匹配）。
    pub fn has_class(&self, class: &str) -> bool {
        self.attr("class")
            .is_some_and(|value| value.split_whitespace().any(|item| item == class))
    }

    pub fn children_nodes(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(|child| match child {
            Child::Node(node) => Some(node),
            Child::Text(_) => None,
        })
    }

    /// 深度优先找第一个满足条件的元素。
    pub fn find(&self, pred: impl Fn(&Node) -> bool + Copy) -> Option<&Node> {
        if pred(self) {
            return Some(self);
        }
        self.children_nodes().find_map(|node| node.find(pred))
    }

    /// 递归取纯文本（实体已解码，空白压成单个空格）。
    pub fn text(&self) -> String {
        let mut raw = String::new();
        self.collect_text(&mut raw);
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn collect_text(&self, out: &mut String) {
        for child in &self.children {
            match child {
                Child::Text(text) => out.push_str(text),
                Child::Node(node) => {
                    if matches!(node.tag.as_str(), "br" | "p" | "div") && !out.ends_with(' ') {
                        out.push(' ');
                    }
                    node.collect_text(out);
                }
            }
        }
    }
}

/// 解析一段 HTML，返回合成的根节点。
///
/// 永远返回一个根：畸形输入只是让树少几个节点，不会失败、不会 panic。
pub fn parse(html: &str) -> Node {
    let mut root = Node::new("#root");
    let mut stack: Vec<Node> = Vec::new();
    let bytes = html.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        let Some(open) = html[cursor..].find('<').map(|i| i + cursor) else {
            // 末尾还有裸文本：不能丢。
            push_text(&mut root, &mut stack, decode(&html[cursor..]));
            break;
        };
        if open > cursor {
            push_text(&mut root, &mut stack, decode(&html[cursor..open]));
        }
        // 注释与 DOCTYPE：整段跳过。
        if html[open..].starts_with("<!--") {
            cursor = html[open..]
                .find("-->")
                .map(|i| open + i + 3)
                .unwrap_or(bytes.len());
            continue;
        }
        if html[open..].starts_with("<!") {
            cursor = html[open..]
                .find('>')
                .map(|i| open + i + 1)
                .unwrap_or(bytes.len());
            continue;
        }
        let Some(close) = html[open..].find('>').map(|i| open + i) else {
            break;
        };
        let raw = &html[open + 1..close];
        cursor = close + 1;

        if let Some(name) = raw.strip_prefix('/') {
            pop(&mut root, &mut stack, name.trim());
            continue;
        }
        let self_closing = raw.ends_with('/');
        let body = raw.trim_end_matches('/');
        let (tag, attrs) = parse_open(body);
        if tag.is_empty() {
            continue;
        }
        let node = Node {
            tag: tag.to_ascii_lowercase(),
            attrs,
            children: Vec::new(),
        };
        // script / style 整体跳过：正文之外的内容，留着只会干扰提取。
        if matches!(node.tag.as_str(), "script" | "style") {
            cursor = skip_element(html, cursor, &node.tag);
            continue;
        }
        if self_closing || is_void(&node.tag) {
            push_node(&mut root, &mut stack, node);
            continue;
        }
        stack.push(node);
    }

    // 收尾：把未闭合的节点按栈序挂回去。
    while let Some(node) = stack.pop() {
        push_node(&mut root, &mut stack, node);
    }
    root
}

fn push_text(root: &mut Node, stack: &mut [Node], text: String) {
    if text.is_empty() {
        return;
    }
    match stack.last_mut() {
        Some(node) => node.children.push(Child::Text(text)),
        None => root.children.push(Child::Text(text)),
    }
}

fn push_node(root: &mut Node, stack: &mut [Node], node: Node) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(Child::Node(node)),
        None => root.children.push(Child::Node(node)),
    }
}

fn pop(root: &mut Node, stack: &mut Vec<Node>, name: &str) {
    let name = name.to_ascii_lowercase();
    let Some(position) = stack.iter().rposition(|node| node.tag == name) else {
        return;
    };
    while stack.len() > position {
        let node = stack.pop().expect("position 已在校验范围内");
        push_node(root, stack, node);
    }
}

/// 解析开始标签，返回 (标签名, 属性)。
fn parse_open(body: &str) -> (String, Vec<(String, String)>) {
    let body = body.trim();
    let end = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
    let tag = &body[..end];
    let mut attrs = Vec::new();
    let mut rest = &body[end..];
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim();
        let value_start = eq + 1;
        let Some(quote) = rest[value_start..].find('"').map(|i| i + value_start) else {
            break;
        };
        let Some(quote_end) = rest[quote + 1..].find('"').map(|i| i + quote + 1) else {
            break;
        };
        if !name.is_empty() && !name.contains('"') {
            attrs.push((
                name.to_ascii_lowercase(),
                decode(&rest[quote + 1..quote_end]),
            ));
        }
        rest = &rest[quote_end + 1..];
    }
    (tag.to_owned(), attrs)
}

/// 找到 `<name …>` 配对的结束位置（用于跳过 script / style）。
fn skip_element(html: &str, from: usize, name: &str) -> usize {
    let closing = format!("</{name}");
    match html[from..].to_ascii_lowercase().find(&closing) {
        Some(index) => {
            let at = from + index;
            html[at..]
                .find('>')
                .map(|i| at + i + 1)
                .unwrap_or(html.len())
        }
        None => html.len(),
    }
}

fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// 解实体：正文里实际出现的几个 + 数字实体。
fn decode(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(semi) = tail.find(';').filter(|index| *index <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "nbsp" => Some(' '),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            _ => entity
                .strip_prefix('#')
                .and_then(|code| {
                    if let Some(hex) = code.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        code.parse::<u32>().ok()
                    }
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_elements_and_attributes() {
        let root = parse(r#"<div class="a"><p id="x">hi</p><img src="u" /></div>"#);
        let div = root.find(|node| node.tag == "div").expect("div 应被解析");
        assert!(div.has_class("a"));
        assert_eq!(div.children_nodes().count(), 2);
        assert_eq!(
            div.children_nodes().next().map(|node| node.text()),
            Some("hi".to_owned())
        );
    }

    #[test]
    fn void_and_self_closing_tags_do_not_swallow_siblings() {
        let root = parse(r#"<div><br><p>a</p><img src="u"></div><span>b</span>"#);
        // 若 br / img 被当容器，span 会被吞进 div 里。
        assert_eq!(root.children_nodes().count(), 2);
        assert_eq!(
            root.children_nodes().nth(1).map(|n| n.tag.as_str()),
            Some("span")
        );
    }

    #[test]
    fn scripts_and_styles_are_skipped_entirely() {
        let root = parse(r#"<div><script>var a = "<p>x</p>";</script><p>正文</p></div>"#);
        assert_eq!(
            root.find(|n| n.tag == "p").map(|n| n.text()),
            Some("正文".to_owned())
        );
        assert!(root.find(|n| n.tag == "script").is_none());
    }

    #[test]
    fn entities_are_decoded() {
        let root = parse(r#"<p>a&nbsp;b &amp; c &lt;d&gt; &#65;&#x42;</p>"#);
        assert_eq!(root.text(), "a b & c <d> AB");
    }

    #[test]
    fn malformed_input_does_not_panic_and_keeps_what_it_can() {
        let root = parse("<div><p>没有闭合<span>内层");
        assert_eq!(
            root.find(|n| n.tag == "span").map(|n| n.text()),
            Some("内层".to_owned())
        );
        assert_eq!(
            root.find(|n| n.tag == "p").map(|n| n.text()),
            Some("没有闭合内层".to_owned())
        );
        // 落单的结束标签不该影响后续解析
        let root = parse("</div><p>ok</p>");
        assert_eq!(
            root.find(|n| n.tag == "p").map(|n| n.text()),
            Some("ok".to_owned())
        );
    }

    #[test]
    fn mismatched_close_pops_to_the_matching_tag() {
        let root = parse("<div><p>a</div><span>b</span>");
        // 结束的 </div> 把 p 一起收掉，span 落在根上
        assert_eq!(root.children_nodes().count(), 2);
    }
}
