//! 检索内核：2-gram BM25 + 候选收敛 / 集合内消歧 + 别名表，以及四组只读查询。
//!
//! 索引只由**页面镜像**构建（见 [`crate::chunk`]）：未镜像的篇目没有块，自然不进检索，
//! `read_doc` 也拿不到。块比旧口径细（一段 / 一个列表项 / 一张表各一块），因此不再按字数丢块。
//!
//! 两条经验写死在实现里（都有实测反例，见调研 §13.11）：
//! 1. **结构化命中用于收敛 / 消歧**——同名节点候选数量随目录内容变化，不按固定上限处理；
//! 2. **别名表按"子树正文"挖掘**——目录文档本身没有正文，其内容在子文档里。

use std::collections::{HashMap, HashSet};

use crate::chunk::{self, Rebuilt};
use crate::model::{
    Block, DETAIL_URL, Doc, DocBrief, DocListGroup, DocText, HitKind, KnowledgeStats,
    NodeCandidate, NodeEntry, NodeFilters, NodeLookup, SCHEMA_VERSION, SearchFilters, SearchHit,
    Side,
};

/// BM25 参数（与 spike 一致）。
const K1: f32 = 1.2;
const B: f32 = 0.6;
/// 别名强度权重：归一化到 0..1 后乘以该系数加到 BM25 分上。
const ALIAS_WEIGHT: f32 = 6.0;
/// 别名命中的文档进入候选集合的门槛。
const ALIAS_CANDIDATE_MIN: f32 = 0.5;
/// 每个标题最多保留多少条别名词条。
const ALIAS_TOP_TERMS: usize = 40;
/// 别名词条的文档频率上限（出现在更多文档里的词太普遍，不是别名）。
const ALIAS_MAX_DF: usize = 8;
/// 别名词条在本标题内的最小出现次数。
const ALIAS_MIN_TF: usize = 3;

/// 虚词表：出现在词条首尾即丢弃（否则会挖出「的仇恨」这类噪声）。
const STOP: &str = "的了是在和与或也都就要不并而及等中上下后前时为对把被让从到有无可以会其与之";

/// 知识库内存实例。协议、进程与 Tauri 无关，GUI 与 MCP 两个入口共用同一份实现。
pub struct KnowledgeBase {
    docs: Vec<Doc>,
    /// 全部块；只有镜像过的篇目才有块。
    blocks: Vec<Block>,
    nodes: Vec<NodeEntry>,
    /// 每个块归属的全部标题（自身 + 祖先），目录文档据此获得子树正文。
    titles_of_block: Vec<Vec<String>>,
    blocks_by_title: HashMap<String, Vec<u32>>,
    /// 2-gram 倒排：词 → [(块序号, 加权词频)]。
    postings: HashMap<String, Vec<(u32, f32)>>,
    lens: Vec<f32>,
    avg_len: f32,
    /// 别名表：词条 → (标题 → 权重)。
    alias: HashMap<String, HashMap<String, f32>>,
    chars: usize,
    /// 已镜像、真正有块的篇目：`read_doc` 只对它们生效。
    indexed: HashSet<String>,
}

impl KnowledgeBase {
    /// 从重建结果建索引（建倒排与别名表）。
    pub fn new(rebuilt: Rebuilt) -> Self {
        let Rebuilt {
            docs,
            blocks,
            nodes,
            chars,
        } = rebuilt;
        let indexed: HashSet<String> = blocks.iter().map(|b| b.path_id.clone()).collect();

        // 块 → 自身标题 + 祖先标题（按面包屑前缀判定）
        let doc_by_path: HashMap<&str, &Doc> =
            docs.iter().map(|d| (d.path_id.as_str(), d)).collect();
        let mut titles_by_breadcrumb: HashMap<Vec<String>, Vec<String>> = HashMap::new();
        for doc in &docs {
            titles_by_breadcrumb
                .entry(doc.breadcrumb.clone())
                .or_default()
                .push(doc.title.clone());
        }
        let titles_of_block: Vec<Vec<String>> = blocks
            .iter()
            .map(|b| {
                let mut titles = vec![b.doc_title.clone()];
                if let Some(doc) = doc_by_path.get(b.path_id.as_str()) {
                    for length in 1..doc.breadcrumb.len() {
                        if let Some(ancestors) = titles_by_breadcrumb.get(&doc.breadcrumb[..length])
                        {
                            titles.extend(ancestors.iter().cloned());
                        }
                    }
                }
                titles
            })
            .collect();

        let mut blocks_by_title: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, titles) in titles_of_block.iter().enumerate() {
            for title in titles {
                blocks_by_title
                    .entry(title.clone())
                    .or_default()
                    .push(i as u32);
            }
        }

        // 2-gram 倒排：面包屑与标题加权 3×，正文 1×。
        let mut postings: HashMap<String, Vec<(u32, f32)>> = HashMap::new();
        let mut lens = Vec::with_capacity(blocks.len());
        for (i, block) in blocks.iter().enumerate() {
            let mut tf: HashMap<String, f32> = HashMap::new();
            for (text, weight) in [
                (block.breadcrumb.join(""), 3.0f32),
                (block.heading_path.join(""), 3.0),
                (block.text.clone(), 1.0),
            ] {
                for gram in grams(&text) {
                    *tf.entry(gram).or_insert(0.0) += weight;
                }
            }
            lens.push(tf.values().sum());
            for (gram, count) in tf {
                postings.entry(gram).or_default().push((i as u32, count));
            }
        }
        let avg_len = if lens.is_empty() {
            1.0
        } else {
            lens.iter().sum::<f32>() / lens.len() as f32
        };

        let top_sections: HashSet<String> = chunk::top_sections(&docs).into_iter().collect();
        let alias = build_alias(&blocks_by_title, &top_sections, &blocks);

        Self {
            docs,
            blocks,
            nodes,
            titles_of_block,
            blocks_by_title,
            postings,
            lens,
            avg_len,
            alias,
            chars,
            indexed,
        }
    }

    pub fn docs(&self) -> &[Doc] {
        &self.docs
    }
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// 已有正文块、实际参加检索的文档；供上层按当前可检索语料估计内容分布。
    pub fn searchable_docs(&self) -> impl Iterator<Item = &Doc> + '_ {
        self.docs
            .iter()
            .filter(|doc| self.indexed.contains(&doc.path_id))
    }

    pub fn nodes(&self) -> &[NodeEntry] {
        &self.nodes
    }

    pub fn stats(&self) -> KnowledgeStats {
        let mut nodes_server = 0;
        let mut nodes_client = 0;
        for node in &self.nodes {
            match node.side {
                Some(Side::Server) => nodes_server += 1,
                Some(Side::Client) => nodes_client += 1,
                _ => {}
            }
        }
        KnowledgeStats {
            schema_version: SCHEMA_VERSION,
            docs: self.docs.len(),
            leaf_docs: self.docs.iter().filter(|d| d.is_leaf).count(),
            mirrored_docs: self.indexed.len(),
            chars: self.chars,
            blocks: self.blocks.len(),
            tables: self.blocks.iter().filter(|b| !b.table.is_empty()).count(),
            nodes: self.nodes.len(),
            nodes_with_params: self.nodes.iter().filter(|n| !n.params.is_empty()).count(),
            nodes_server,
            nodes_client,
        }
    }

    // ── 只读查询（对外暴露的工具面） ───────────────────────────────────────

    /// 批量检索：每个查询返回 top-k 命中（§13.3 的 `search`）。
    pub fn search(&self, queries: &[String], top_k: usize) -> Vec<Vec<SearchHit>> {
        queries.iter().map(|q| self.retrieve(q, top_k)).collect()
    }

    /// 带约束的检索原语：先用 BM25 / 节点 / 文档别名召回，再应用调用方给定的元数据约束。
    ///
    /// 返回的块带有标题路径、锚点和原文链接，插件可将其作为有出处的上下文。
    /// 约束严格生效，过滤后没有结果时不会静默扩大范围；场景识别与多路检索由插件负责。
    pub fn search_scoped(
        &self,
        queries: &[String],
        top_k: usize,
        filters: &SearchFilters,
    ) -> Vec<Vec<SearchHit>> {
        queries
            .iter()
            .map(|query| self.retrieve_scoped(query, top_k, filters))
            .collect()
    }

    /// `get_node`：按节点名返回**候选集合**。
    ///
    /// 同名节点可能有多份，工具不替调用方猜一个。
    pub fn get_node(&self, names: &[String]) -> Vec<NodeLookup> {
        names.iter().map(|raw| self.lookup_node(raw)).collect()
    }

    /// 按运行端和节点图类别检索节点；Both 节点同时适用于服务端与客户端筛选。
    pub fn get_node_scoped(&self, names: &[String], filters: &NodeFilters) -> Vec<NodeLookup> {
        self.get_node(names)
            .into_iter()
            .map(|mut group| {
                group.matches.retain(|candidate| {
                    let side_ok = filters.side.is_none_or(|side| {
                        candidate.side == Some(side) || candidate.side == Some(Side::Both)
                    });
                    let graph_ok = filters.node_graph.as_ref().is_none_or(|graph| {
                        candidate.node_graph.as_ref().is_some_and(|value| {
                            value.contains(graph) || chunk::is_subsequence(graph, value)
                        })
                    });
                    side_ok && graph_ok
                });
                group
            })
            .collect()
    }

    /// 在候选集合内按整句线索收窄（问题里出现"服务端 / 客户端"，或出现候选所属文档名）。
    ///
    /// MCP 的工具入参只有节点名，因此这一步属于**调用方**的事；本方法供 GUI 自检与评测使用。
    pub fn get_node_narrowed(&self, names: &[String], question: &str) -> Vec<NodeLookup> {
        names
            .iter()
            .map(|raw| {
                let mut lookup = self.lookup_node(raw);
                let by_doc: Vec<NodeCandidate> = lookup
                    .matches
                    .iter()
                    .filter(|c| question.contains(&c.doc_title))
                    .cloned()
                    .collect();
                if !by_doc.is_empty() {
                    lookup.matches = by_doc;
                }
                if let Some(side) = side_hint(question) {
                    let by_side: Vec<NodeCandidate> = lookup
                        .matches
                        .iter()
                        .filter(|c| c.side == Some(side))
                        .cloned()
                        .collect();
                    if !by_side.is_empty() {
                        lookup.matches = by_side;
                    }
                }
                lookup
            })
            .collect()
    }

    /// `list_docs`：按关键词列标题（不含正文），用于消歧与"有没有相关文档"。
    pub fn list_docs(&self, keywords: &[String]) -> Vec<DocListGroup> {
        if keywords.is_empty() {
            return vec![DocListGroup {
                keyword: String::new(),
                total: self.docs.len(),
                docs: self.docs.iter().map(brief).collect(),
            }];
        }
        keywords
            .iter()
            .map(|keyword| {
                let kw = keyword.trim();
                let docs: Vec<DocBrief> = self
                    .docs
                    .iter()
                    .filter(|d| d.title.contains(kw) || chunk::is_subsequence(kw, &d.title))
                    .map(brief)
                    .collect();
                DocListGroup {
                    keyword: keyword.clone(),
                    total: docs.len(),
                    docs,
                }
            })
            .collect()
    }

    /// `read_doc`：按 `path_id` 或标题取整篇（可选从某个锚点起）。
    ///
    /// 未镜像的篇目没有块、不进索引，按"没有这篇"处理（界面会提示先镜像）。
    pub fn read_doc(&self, key: &str, from_anchor: Option<&str>) -> Option<DocText> {
        let doc = self
            .docs
            .iter()
            .find(|d| d.path_id == key)
            .or_else(|| self.docs.iter().find(|d| d.title == key))
            .or_else(|| self.docs.iter().find(|d| d.title.contains(key)))?;
        if !self.indexed.contains(&doc.path_id) {
            return None;
        }
        let doc_blocks: Vec<&Block> = self
            .blocks
            .iter()
            .filter(|b| b.path_id == doc.path_id)
            .collect();
        let mut hits: Vec<SearchHit> = doc_blocks
            .iter()
            .map(|b| self.hit(b, HitKind::Sparse))
            .collect();
        if let Some(anchor) = from_anchor
            && let Some(pos) = hits.iter().position(|h| h.anchor == anchor)
        {
            hits.drain(..pos);
        }
        Some(DocText {
            path_id: doc.path_id.clone(),
            title: doc.title.clone(),
            url: doc.url.clone(),
            updated_at: doc.updated_at.clone(),
            side: doc.side,
            node_graph: doc.node_graph.clone(),
            has_tables: doc_blocks.iter().any(|b| !b.table.is_empty()),
            blocks: hits,
        })
    }

    // ── 内部 ───────────────────────────────────────────────────────────────

    fn lookup_node(&self, raw: &str) -> NodeLookup {
        let name = raw.trim();
        let mut hits: Vec<&NodeEntry> = self.nodes.iter().filter(|n| n.name == name).collect();
        if hits.is_empty() {
            hits = self
                .nodes
                .iter()
                .filter(|n| n.name.contains(name) || chunk::is_subsequence(name, &n.name))
                .collect();
        }
        NodeLookup {
            query: raw.to_owned(),
            matches: hits.into_iter().map(|n| self.candidate(n)).collect(),
        }
    }

    fn retrieve(&self, query: &str, top_k: usize) -> Vec<SearchHit> {
        self.retrieve_scoped(query, top_k, &SearchFilters::default())
    }

    fn retrieve_scoped(
        &self,
        query: &str,
        top_k: usize,
        filters: &SearchFilters,
    ) -> Vec<SearchHit> {
        let bm = self.bm25(query);
        let (mut candidates, node_blocks) = self.converge(query);
        let side = side_hint(query);
        let alias = self.alias_scores(query);

        // 别名命中的文档整体进候选：目录文档靠这一步把子树块带进来。
        let mut candidate_set: HashSet<u32> = candidates.iter().copied().collect();
        for (title, value) in &alias.by_title {
            if *value >= ALIAS_CANDIDATE_MIN {
                for idx in self.blocks_by_title.get(title).into_iter().flatten() {
                    candidate_set.insert(*idx);
                }
            }
        }
        candidates = candidate_set.iter().copied().collect();

        let matches_filter = |block: &Block| {
            let doc_ok = filters
                .doc_path_id
                .as_ref()
                .is_none_or(|path_id| block.path_id == *path_id);
            let section_ok = filters.section.as_ref().is_none_or(|section| {
                block.section == *section
                    || block
                        .breadcrumb
                        .iter()
                        .any(|part| part == section || chunk::is_subsequence(section, part))
            });
            let side_ok = filters
                .side
                .is_none_or(|side| block.side == Some(side) || block.side == Some(Side::Both));
            let graph_ok = filters.node_graph.as_ref().is_none_or(|graph| {
                block.node_graph.as_ref().is_some_and(|value| {
                    value == graph || value.contains(graph) || chunk::is_subsequence(graph, value)
                })
            });
            let tables_ok = !filters.tables_only || !block.table.is_empty();
            doc_ok && section_ok && side_ok && graph_ok && tables_ok
        };

        let score = |idx: u32| -> f32 {
            let block = &self.blocks[idx as usize];
            let mut value = bm.get(&idx).copied().unwrap_or(0.0);
            if node_blocks.contains(&idx) {
                value += 3.0;
            }
            if let Some(side) = side {
                match block.side {
                    Some(current) if current == side => value += 2.0,
                    Some(_) => value -= 1.5,
                    None => {}
                }
            }
            value + ALIAS_WEIGHT * alias.by_block[idx as usize]
        };
        let by_score = |a: &u32, b: &u32| {
            score(*b)
                .partial_cmp(&score(*a))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(b))
        };

        // 块是"一段 / 一个列表项 / 一张表"这一粒度，同一个标题下会有多个块；
        // 因此这里按块去重（候选集合本身就是块序号，天然唯一），不再按锚点合并。
        let mut out: Vec<SearchHit> = Vec::with_capacity(top_k);
        candidates.retain(|idx| matches_filter(&self.blocks[*idx as usize]));
        candidates.retain(|idx| score(*idx) > 0.0);
        candidates.sort_by(by_score);
        for idx in &candidates {
            let block = &self.blocks[*idx as usize];
            let kind = if node_blocks.contains(idx) {
                HitKind::CandidateNode
            } else {
                HitKind::CandidateDoc
            };
            out.push(self.hit(block, kind));
            if out.len() >= top_k {
                break;
            }
        }
        if out.len() < top_k {
            let in_candidates: HashSet<u32> = candidates.iter().copied().collect();
            let mut rest: Vec<u32> = (0..self.blocks.len() as u32)
                .filter(|idx| {
                    !in_candidates.contains(idx)
                        && matches_filter(&self.blocks[*idx as usize])
                        && score(*idx) > 0.0
                })
                .collect();
            rest.sort_by(by_score);
            for idx in rest {
                let kind = HitKind::Sparse;
                out.push(self.hit(&self.blocks[idx as usize], kind));
                if out.len() >= top_k {
                    break;
                }
            }
        }
        out
    }

    fn bm25(&self, query: &str) -> HashMap<u32, f32> {
        let mut scores: HashMap<u32, f32> = HashMap::new();
        let total = self.blocks.len() as f32;
        for gram in grams(query).into_iter().collect::<HashSet<_>>() {
            let Some(posting) = self.postings.get(&gram) else {
                continue;
            };
            let df = posting.len() as f32;
            let idf = (1.0 + (total - df + 0.5) / (df + 0.5)).ln();
            for (idx, tf) in posting {
                let len = self.lens[*idx as usize];
                let denominator = *tf + K1 * (1.0 - B + B * len / self.avg_len);
                *scores.entry(*idx).or_insert(0.0) += idf * (tf * (K1 + 1.0)) / denominator;
            }
        }
        scores
    }

    /// 候选收敛：命中的节点名 / 文档标题（含祖先标题，因此目录文档也能命中）。
    fn converge(&self, query: &str) -> (Vec<u32>, HashSet<u32>) {
        let mut name: Option<&str> = None;
        for node in &self.nodes {
            if query.contains(&node.name)
                && name.is_none_or(|current| node.name.len() > current.len())
            {
                name = Some(&node.name);
            }
        }
        let mut title: Option<&String> = None;
        for key in self.blocks_by_title.keys() {
            if query.contains(key.as_str()) && title.is_none_or(|current| key.len() > current.len())
            {
                title = Some(key);
            }
        }

        let mut candidates: Vec<u32> = Vec::new();
        let mut node_blocks: HashSet<u32> = HashSet::new();
        if let Some(name) = name {
            for (i, block) in self.blocks.iter().enumerate() {
                if block.heading_path.iter().any(|h| h == name) {
                    candidates.push(i as u32);
                    node_blocks.insert(i as u32);
                }
            }
        }
        if let Some(title) = title {
            for idx in self.blocks_by_title.get(title).into_iter().flatten() {
                candidates.push(*idx);
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        (candidates, node_blocks)
    }

    fn alias_scores(&self, query: &str) -> AliasScores {
        let mut by_title: HashMap<String, f32> = HashMap::new();
        for n in [2usize, 3, 4] {
            for gram in ngrams(query, n) {
                if let Some(entry) = self.alias.get(&gram) {
                    for (title, weight) in entry {
                        *by_title.entry(title.clone()).or_insert(0.0) += weight;
                    }
                }
            }
        }
        let max = by_title.values().copied().fold(0.0f32, f32::max).max(1e-9);
        for value in by_title.values_mut() {
            *value /= max;
        }
        let mut by_block = vec![0.0f32; self.blocks.len()];
        for (i, titles) in self.titles_of_block.iter().enumerate() {
            by_block[i] = titles
                .iter()
                .map(|t| by_title.get(t).copied().unwrap_or(0.0))
                .fold(0.0, f32::max);
        }
        AliasScores { by_title, by_block }
    }

    fn hit(&self, block: &Block, kind: HitKind) -> SearchHit {
        SearchHit {
            path_id: block.path_id.clone(),
            doc_title: block.doc_title.clone(),
            anchor: block.anchor.clone(),
            breadcrumb: block.breadcrumb.clone(),
            text: block.text.clone(),
            side: block.side,
            node_graph: block.node_graph.clone(),
            updated_at: block.updated_at.clone(),
            url: block.url.clone(),
            kind,
            table: block.table.clone(),
        }
    }

    fn candidate(&self, node: &NodeEntry) -> NodeCandidate {
        let breadcrumb = self
            .blocks
            .iter()
            .find(|b| b.anchor == node.anchor)
            .map(|b| b.breadcrumb.clone())
            .unwrap_or_default();
        NodeCandidate {
            name: node.name.clone(),
            path_id: node.path_id.clone(),
            doc_title: node.doc_title.clone(),
            anchor: node.anchor.clone(),
            node_graph: node.node_graph.clone(),
            side: node.side,
            breadcrumb,
            params: node.params.clone(),
            params_need_render: node.params_need_render,
            url: self
                .docs
                .iter()
                .find(|doc| doc.path_id == node.path_id)
                .map(|doc| doc.url.clone())
                .unwrap_or_else(|| format!("{DETAIL_URL}/{}", node.path_id)),
        }
    }
}

struct AliasScores {
    by_title: HashMap<String, f32>,
    by_block: Vec<f32>,
}

fn brief(doc: &Doc) -> DocBrief {
    DocBrief {
        path_id: doc.path_id.clone(),
        title: doc.title.clone(),
        breadcrumb: doc.breadcrumb.clone(),
        section: doc.section.clone(),
        is_leaf: doc.is_leaf,
        updated_at: doc.updated_at.clone(),
    }
}

fn side_hint(query: &str) -> Option<Side> {
    if query.contains("客户端") {
        Some(Side::Client)
    } else if query.contains("服务端") {
        Some(Side::Server)
    } else {
        None
    }
}

/// 2-gram 切分：去掉全部空白后按 2 字滑窗；长度 1 时返回该字（与 spike 一致）。
fn grams(text: &str) -> Vec<String> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let chars: Vec<char> = compact.chars().collect();
    if chars.len() == 1 {
        return vec![compact];
    }
    (0..chars.len().saturating_sub(1))
        .map(|i| chars[i..i + 2].iter().collect())
        .collect()
}

/// 别名挖掘用的 n-gram（先去掉空白与常见标点）。
fn ngrams(text: &str, n: usize) -> Vec<String> {
    let compact: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && !"，。、；：（）()【】《》\"'!！?？,.:;[]".contains(*c))
        .collect();
    let chars: Vec<char> = compact.chars().collect();
    if chars.len() < n {
        return Vec::new();
    }
    (0..=chars.len() - n)
        .map(|i| chars[i..i + n].iter().collect())
        .collect()
}

fn is_stop(c: char) -> bool {
    STOP.contains(c)
}

/// 从「标题（含子树正文）」自动挖掘别名：文档特异的 2–4 字词。
fn build_alias(
    blocks_by_title: &HashMap<String, Vec<u32>>,
    top_sections: &HashSet<String>,
    blocks: &[Block],
) -> HashMap<String, HashMap<String, f32>> {
    let mut term_tf: Vec<(&str, HashMap<String, usize>)> = Vec::new();
    for (title, idxs) in blocks_by_title {
        if top_sections.contains(title) {
            continue; // 顶层分区太泛，不做别名
        }
        let mut text = String::new();
        for idx in idxs {
            let block = &blocks[*idx as usize];
            text.push_str(&block.heading_path.join(""));
            text.push_str(&block.text);
        }
        let mut counts: HashMap<String, usize> = HashMap::new();
        for n in [2usize, 3, 4] {
            for gram in ngrams(&text, n) {
                let chars: Vec<char> = gram.chars().collect();
                if chars.iter().all(|c| is_stop(*c)) {
                    continue;
                }
                if is_stop(chars[0]) || is_stop(chars[chars.len() - 1]) {
                    continue;
                }
                *counts.entry(gram).or_insert(0) += 1;
            }
        }
        term_tf.push((title.as_str(), counts));
    }

    let mut df: HashMap<&str, usize> = HashMap::new();
    for (_, counts) in &term_tf {
        for term in counts.keys() {
            *df.entry(term.as_str()).or_insert(0) += 1;
        }
    }

    let total_titles = term_tf.len().max(1) as f32;
    let mut alias: HashMap<String, HashMap<String, f32>> = HashMap::new();
    for (title, counts) in &term_tf {
        let total: usize = counts.values().sum();
        if total == 0 {
            continue;
        }
        let mut scored: Vec<(&String, f32)> = counts
            .iter()
            .filter(|(term, tf)| {
                df.get(term.as_str()).copied().unwrap_or(1) <= ALIAS_MAX_DF && **tf >= ALIAS_MIN_TF
            })
            .map(|(term, tf)| {
                let doc_freq = df.get(term.as_str()).copied().unwrap_or(1) as f32;
                (
                    term,
                    (*tf as f32 / total as f32) * (1.0 + total_titles / doc_freq).ln() * 1000.0,
                )
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (term, weight) in scored.into_iter().take(ALIAS_TOP_TERMS) {
            alias
                .entry(term.clone())
                .or_default()
                .insert((*title).to_owned(), weight);
        }
    }
    alias
}
