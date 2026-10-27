//! 知识库插件的 RAG 检索规划：根据实际召回内容选择一条补充检索路径。
//!
//! 内核负责召回与过滤。本模块只使用索引中的分区、节点图、端侧和表格信息，
//! 不维护随知识库内容变化而失效的场景关键词表。

use std::collections::{HashMap, HashSet};

use crate::{Doc, KnowledgeBase, SearchFilters, SearchHit, Side};

const MIN_POOL: usize = 16;
const MAX_POOL: usize = 60;
const EVIDENCE_DOCS: usize = 8;
const MIN_EVIDENCE_DOCS: usize = 4;
const RRF_OFFSET: f32 = 60.0;

type HitKey = (String, String, String);

/// 普通问题直接使用内核检索；证据足够集中时，再执行一次定向检索。
/// 桌面端和 MCP 入口调用同一规划器。
pub fn plan_rag_search(
    kb: &KnowledgeBase,
    queries: &[String],
    top_k: usize,
) -> Vec<Vec<SearchHit>> {
    queries
        .iter()
        .map(|query| plan_one(kb, query, top_k))
        .collect()
}

fn plan_one(kb: &KnowledgeBase, query: &str, top_k: usize) -> Vec<SearchHit> {
    if top_k == 0 {
        return Vec::new();
    }

    // 多取一些原始命中作为判断依据，但只观察不同文档，避免一篇长文的多个块主导规划。
    let pool = top_k.saturating_mul(3).clamp(MIN_POOL, MAX_POOL);
    let broad = kb
        .search(&[query.to_owned()], pool)
        .pop()
        .unwrap_or_default();
    let searchable_docs: Vec<&Doc> = kb.searchable_docs().collect();
    let Some(filters) = choose_focus(&searchable_docs, &broad) else {
        return broad.into_iter().take(top_k).collect();
    };

    // 至多补充一条路径，查询向量最多额外计算一次。使用原查询，不向问题注入预设词。
    let focused = kb
        .search_scoped(&[query.to_owned()], pool, &filters)
        .pop()
        .unwrap_or_default();
    let mut ranked = Vec::new();
    let mut positions = HashMap::new();
    merge_route(&mut ranked, &mut positions, &broad);
    merge_route(&mut ranked, &mut positions, &focused);
    ranked.sort_by(|left, right| right.0.total_cmp(&left.0).then(left.1.cmp(&right.1)));

    // 保留至少一半的宽召回命中；证据判断失误时，定向路径不能盖掉通用结果。
    let broad_floor = top_k.div_ceil(2);
    let mut selected = Vec::with_capacity(top_k);
    let mut seen = HashSet::new();
    for hit in broad.into_iter().take(broad_floor) {
        seen.insert(hit_key(&hit));
        selected.push(hit);
    }
    for (_, _, hit) in ranked {
        if selected.len() >= top_k {
            break;
        }
        if seen.insert(hit_key(&hit)) {
            selected.push(hit);
        }
    }
    selected
}

/// 只从初步命中的不同文档中选取一种高一致性的内容约束。
fn choose_focus(docs: &[&Doc], broad: &[SearchHit]) -> Option<SearchFilters> {
    let docs_by_path: HashMap<&str, &Doc> = docs
        .iter()
        .map(|doc| (doc.path_id.as_str(), *doc))
        .collect();
    let mut observed = HashSet::new();
    let mut sections: HashMap<&str, usize> = HashMap::new();
    let mut graphs: HashMap<&str, usize> = HashMap::new();
    let mut server = 0;
    let mut client = 0;
    let mut tables = 0;

    for hit in broad {
        if observed.len() >= EVIDENCE_DOCS {
            break;
        }
        let Some(doc) = docs_by_path.get(hit.path_id.as_str()) else {
            continue;
        };
        if !observed.insert(hit.path_id.as_str()) {
            continue;
        }
        if !doc.section.is_empty() {
            *sections.entry(doc.section.as_str()).or_default() += 1;
        }
        if let Some(graph) = hit.node_graph.as_deref().or(doc.node_graph.as_deref()) {
            *graphs.entry(graph).or_default() += 1;
        }
        match hit.side.or(doc.side) {
            Some(Side::Server) => server += 1,
            Some(Side::Client) => client += 1,
            _ => {}
        }
        if !hit.table.is_empty() {
            tables += 1;
        }
    }

    if observed.len() < MIN_EVIDENCE_DOCS {
        return None;
    }
    let threshold = (observed.len() * 2).div_ceil(3).max(3);
    let mut best: Option<(usize, u8, SearchFilters)> = None;
    let corpus_size = docs.len();

    for (graph, count) in graphs {
        let corpus_count = docs
            .iter()
            .filter(|doc| doc.node_graph.as_deref() == Some(graph))
            .count();
        consider(
            &mut best,
            count,
            threshold,
            enriched(count, observed.len(), corpus_count, corpus_size),
            4,
            SearchFilters {
                node_graph: Some(graph.to_owned()),
                ..Default::default()
            },
        );
    }
    consider(
        &mut best,
        server,
        threshold,
        enriched(
            server,
            observed.len(),
            docs.iter()
                .filter(|doc| doc.side == Some(Side::Server))
                .count(),
            corpus_size,
        ),
        3,
        SearchFilters {
            side: Some(Side::Server),
            ..Default::default()
        },
    );
    consider(
        &mut best,
        client,
        threshold,
        enriched(
            client,
            observed.len(),
            docs.iter()
                .filter(|doc| doc.side == Some(Side::Client))
                .count(),
            corpus_size,
        ),
        3,
        SearchFilters {
            side: Some(Side::Client),
            ..Default::default()
        },
    );
    consider(
        &mut best,
        tables,
        threshold,
        true,
        2,
        SearchFilters {
            tables_only: true,
            ..Default::default()
        },
    );
    for (section, count) in sections {
        let corpus_count = docs
            .iter()
            .filter(|doc| doc.section.as_str() == section)
            .count();
        consider(
            &mut best,
            count,
            threshold,
            enriched(count, observed.len(), corpus_count, corpus_size),
            1,
            SearchFilters {
                section: Some(section.to_owned()),
                ..Default::default()
            },
        );
    }
    best.map(|(_, _, filters)| filters)
}

fn consider(
    best: &mut Option<(usize, u8, SearchFilters)>,
    count: usize,
    threshold: usize,
    is_enriched: bool,
    priority: u8,
    filters: SearchFilters,
) {
    if count >= threshold
        && is_enriched
        && best.as_ref().is_none_or(|(best_count, best_priority, _)| {
            (count, priority) > (*best_count, *best_priority)
        })
    {
        *best = Some((count, priority, filters));
    }
}

/// 候选在初步结果中的占比至少是整个目录中的两倍，才值得追加定向检索。
fn enriched(count: usize, sample_size: usize, corpus_count: usize, corpus_size: usize) -> bool {
    corpus_count > 0
        && count.saturating_mul(corpus_size)
            >= sample_size.saturating_mul(corpus_count).saturating_mul(2)
}

fn merge_route(
    ranked: &mut Vec<(f32, usize, SearchHit)>,
    positions: &mut HashMap<HitKey, usize>,
    hits: &[SearchHit],
) {
    for (rank, hit) in hits.iter().enumerate() {
        let key = hit_key(hit);
        let contribution = 1.0 / (RRF_OFFSET + rank as f32 + 1.0);
        if let Some(position) = positions.get(&key).copied() {
            ranked[position].0 += contribution;
        } else {
            let position = ranked.len();
            positions.insert(key, position);
            ranked.push((contribution, position, hit.clone()));
        }
    }
}

fn hit_key(hit: &SearchHit) -> HitKey {
    (hit.path_id.clone(), hit.anchor.clone(), hit.text.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HitKind;

    fn doc(id: usize, section: &str) -> Doc {
        Doc {
            path_id: id.to_string(),
            title: format!("文档{id}"),
            section: section.to_owned(),
            breadcrumb: vec![section.to_owned()],
            is_leaf: true,
            updated_at: None,
            node_graph: None,
            side: None,
            url: String::new(),
        }
    }

    fn hit(id: usize, table: bool) -> SearchHit {
        SearchHit {
            path_id: id.to_string(),
            doc_title: format!("文档{id}"),
            anchor: format!("anchor-{id}"),
            breadcrumb: Vec::new(),
            text: format!("命中{id}"),
            side: None,
            node_graph: None,
            updated_at: None,
            url: String::new(),
            kind: HitKind::Sparse,
            table: if table {
                vec![vec!["列".to_owned()]]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn focuses_on_an_enriched_section_without_query_keywords() {
        let docs: Vec<Doc> = (0..8)
            .map(|id| {
                doc(
                    id,
                    if id < 3 {
                        "少数分区"
                    } else {
                        "多数分区"
                    },
                )
            })
            .collect();
        let broad = vec![hit(0, false), hit(1, false), hit(2, false), hit(3, false)];
        let searchable: Vec<&Doc> = docs.iter().collect();
        let focus = choose_focus(&searchable, &broad).expect("少数分区在命中中明显集中");
        assert_eq!(focus.section.as_deref(), Some("少数分区"));
    }

    #[test]
    fn dominant_corpus_section_does_not_trigger_an_extra_search() {
        let docs: Vec<Doc> = (0..8)
            .map(|id| {
                doc(
                    id,
                    if id < 3 {
                        "少数分区"
                    } else {
                        "多数分区"
                    },
                )
            })
            .collect();
        let broad = vec![hit(3, false), hit(4, false), hit(5, false), hit(6, false)];
        let searchable: Vec<&Doc> = docs.iter().collect();
        assert!(choose_focus(&searchable, &broad).is_none());
    }

    #[test]
    fn table_evidence_can_select_table_search_without_a_preset_term() {
        let docs: Vec<Doc> = (0..8)
            .map(|id| {
                doc(
                    id,
                    if id < 3 {
                        "少数分区"
                    } else {
                        "多数分区"
                    },
                )
            })
            .collect();
        let broad = vec![hit(3, true), hit(4, true), hit(5, true), hit(6, false)];
        let searchable: Vec<&Doc> = docs.iter().collect();
        let focus = choose_focus(&searchable, &broad).expect("表格命中占多数");
        assert!(focus.tables_only);
    }
}
