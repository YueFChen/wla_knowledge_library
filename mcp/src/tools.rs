//! 只读工具面：结构化查询、普通检索、规划式 RAG 与 Lua API 定向检索。
//!
//! 三条硬约束（调研 §13.3、ADR-010）：
//! 1. **全部只读**：`readOnlyHint: true`、无副作用、不接触密钥；
//! 2. **输入与输出都要声明 schema**（对外暴露的 Tool 强制，ADR-010）；
//! 3. **局限声明写在 description 里**——外部客户端的体验我们控制不了，工具描述是唯一通道：
//!    正文图片只记地址不落盘、可能落后官网、未命中就回"知识库中没有"、
//!    **查询失败不等于没有资料**、**没镜像的篇目不在索引里**。
//!
//! 内容源是页面镜像：节点参数与检索命中均保留表格行列结构，作为 RAG 上下文返回。
//! `rag_search` 的复杂检索路由由知识库插件规划；这里仅负责 MCP 参数校验与协议结果封装。

use serde_json::{Value, json};
use wonderland_knowledge::{KnowledgeBase, NodeFilters, Side};
use wonderland_knowledge_library::plan_rag_search;

/// 工具清单（`tools/list` 的返回体）。
pub fn list() -> Value {
    let mut result = json!({
        "tools": [
            {
                "name": "get_node",
                "description": "按节点名查千星奇域节点说明和参数。可按服务端/客户端及节点图类别筛选，支持子串与子序列匹配和批量查询。结构化节点查询优先于通用 search。参数表含表头行；同名节点会返回所有符合筛选条件的候选及来源，不会替调用方猜选。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "names": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "节点名列表（如「建立字典」「受到攻击时」），多个节点合并为一次调用"
                        },
                        "side": { "type": "string", "enum": ["server", "client"], "description": "可选归属端过滤；双端节点会同时包含" },
                        "node_graph": { "type": "string", "description": "可选节点图类别过滤，例如角色技能节点图" }
                    },
                    "required": ["names"],
                    "additionalProperties": false
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "results": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string" },
                                    "matches": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "name": { "type": "string" },
                                                "path_id": { "type": "string" },
                                                "doc_title": { "type": "string" },
                                                "breadcrumb": { "type": "array", "items": { "type": "string" } },
                                                "anchor": { "type": "string" },
                                                "side": { "type": ["string", "null"], "description": "server / client / both" },
                                                "node_graph": { "type": ["string", "null"] },
                                                "params": {
                                                    "type": "array",
                                                    "description": "参数表（含表头行，来自官方页面镜像）；没取到参数表时为空数组",
                                                    "items": { "type": "array", "items": { "type": "string" } }
                                                },
                                                "params_need_render": { "type": "boolean", "description": "true = 镜像里没有参数表，需对照原文" },
                                                "url": { "type": "string" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "annotations": { "title": "节点查询", "readOnlyHint": true, "openWorldHint": false }
            },
            {
                "name": "list_docs",
                "description": "列出知识库里的文档标题（不含正文），用于消歧与确认是否有相关文档。不传 keywords 则列出当前目录里的全部标题。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "keywords": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "过滤关键词列表，对标题做子串 / 子序列匹配；省略或空数组表示全部"
                        }
                    },
                    "additionalProperties": false
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "results": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "keyword": { "type": "string" },
                                    "total": { "type": "integer" },
                                    "docs": { "type": "array", "items": { "type": "object" } }
                                }
                            }
                        }
                    }
                },
                "annotations": { "title": "文档清单", "readOnlyHint": true, "openWorldHint": false }
            },
            {
                "name": "read_doc",
                "description": "按 path_id 或标题读取整篇文档（可从一个锚点开始），返回分块正文、标题层级路径和结构化表格。**不做截断**；正文来自官方页面镜像，正文图片只记地址、不落盘。**没镜像的篇目不在知识库索引里**，此时会提示没有匹配的文档。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "文档 path_id（如 mhw66orrrfkm）或标题（如「仇恨配置」）" },
                        "from_anchor": { "type": "string", "description": "可选的起始锚点，用于从某一段开始读" }
                    },
                    "required": ["key"],
                    "additionalProperties": false
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "path_id": { "type": "string" },
                        "title": { "type": "string" },
                        "url": { "type": "string" },
                        "has_tables": { "type": "boolean", "description": "该篇是否含结构化表格" },
                        "blocks": { "type": "array", "items": { "type": "object" } }
                    }
                },
                "annotations": { "title": "读文档", "readOnlyHint": true, "openWorldHint": false }
            },
            {
                "name": "rag_search",
                "description": "检索千星奇域本地官方页面镜像，为回答提供带出处的 RAG 上下文。直接用自然语言描述问题；知识库插件先宽召回，再依据实际命中的目录、节点图、端侧与表格分布决定是否补充定向检索，并合并结果。返回标题路径、锚点、原文链接及表格结构。节点名明确时优先用 get_node，文档明确时用 read_doc。镜像可能落后官网，未镜像篇目不进正文索引。空结果表示本地索引未命中；工具报错不表示没有资料。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "queries": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "自然语言问题列表，越具体越准；多个独立问题合并为一次调用"
                        },
                        "top_k": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 20,
                            "description": "每个查询最多返回多少条，默认 5"
                        },
                    },
                    "required": ["queries"],
                    "additionalProperties": false
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "results": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string" },
                                    "hits": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "path_id": { "type": "string" },
                                                "doc_title": { "type": "string" },
                                                "breadcrumb": { "type": "array", "items": { "type": "string" } },
                                                "anchor": { "type": "string" },
                                                "side": { "type": ["string", "null"] },
                                                "node_graph": { "type": ["string", "null"] },
                                                "updated_at": { "type": ["string", "null"] },
                                                "url": { "type": "string" },
                                                "text": { "type": "string" },
                                                "table": { "type": "array", "description": "命中块的结构化表格；非表格块为空数组", "items": { "type": "array", "items": { "type": "string" } } }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "annotations": { "title": "知识库 RAG 检索", "readOnlyHint": true, "openWorldHint": false }
            }
        ]
    });
    let rag = result["tools"][3].clone();
    let mut basic = rag.clone();
    basic["name"] = json!("search");
    basic["description"] = json!(
        "在千星奇域本地官方页面镜像中执行基础稀疏检索，不进行场景化多路规划。适合直接查找已知概念或获取基础候选。开放式问题或需要按命中内容追加定向检索时使用 rag_search；节点名明确时优先用 get_node。结果含文档路径、锚点、原文链接与表格结构。镜像可能落后官网，未镜像篇目不进正文索引。"
    );
    basic["annotations"]["title"] = json!("基础检索");
    basic["inputSchema"]["properties"]["queries"]["description"] =
        json!("一个或多个关键词、名称或简短查询句");
    let mut rag = rag;
    rag["description"] = json!(
        "面向开放问题与排障的专门 RAG 检索。知识库插件先宽召回，再依据实际命中的目录、节点图、端侧与表格分布决定是否补充定向检索，并合并结果。返回标题路径、锚点、原文链接及表格结构。节点名明确时优先用 get_node，文档明确时用 read_doc。镜像可能落后官网，未镜像篇目不进正文索引；空结果表示本地索引未命中，工具报错不表示没有资料。"
    );
    let mut lua = rag.clone();
    lua["name"] = json!("search_lua_api");
    lua["description"] = json!(
        "专门检索千星奇域《客户端控件API文档》中的客户端 Lua UI 脚本接口，包括 API 名称、函数签名、参数、返回值、类型、字段、方法和枚举。优先返回接口表格，只在表格未命中时回退到该 API 文档的正文。传入 API 标识符或具体问题；不会从其他教程文档扩展结果。结果含原文锚点和链接。"
    );
    lua["annotations"]["title"] = json!("Lua API 接口检索");
    lua["inputSchema"]["properties"]["queries"]["description"] =
        json!("Lua API 名称、函数/方法名、类型名或描述该接口用途的问题，可批量提交");
    lua["outputSchema"]["properties"]["source"] =
        json!({ "type": "string", "description": "命中的专用 API 文档标题" });
    let tools = result["tools"].as_array_mut().expect("tools array");
    tools[3] = basic;
    tools.push(rag);
    tools.push(lua);
    result
}

/// 工具执行。错误按 MCP 约定**放进结果里**（`isError: true`），而不是协议级错误，
/// 这样模型能看到失败原因并换词重试（调研 §6.4 的"查询失败 ≠ 没有资料"）。
pub fn call(kb: &KnowledgeBase, name: &str, arguments: &Value) -> Result<Value, String> {
    let arguments = if arguments.is_null() {
        &json!({})
    } else {
        arguments
    };
    let result = match name {
        "get_node" => get_node(kb, arguments)?,
        "list_docs" => list_docs(kb, arguments)?,
        "read_doc" => read_doc(kb, arguments)?,
        "search" => search(kb, arguments)?,
        "rag_search" => rag_search(kb, arguments)?,
        "search_lua_api" => search_lua_api(kb, arguments)?,
        other => return Err(format!("未知工具：{other}")),
    };
    let text = result.to_string();
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": result
    }))
}

fn strings(args: &Value, field: &str) -> Option<Vec<String>> {
    args.get(field)?.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect()
    })
}

/// 取必填的非空字符串列表；空数组按"没有可用输入"处理（服务端无法回答空查询）。
fn required_strings(args: &Value, field: &str) -> Result<Vec<String>, String> {
    let values = strings(args, field).unwrap_or_default();
    if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
        return Err(format!("{field} 至少要有一个非空项"));
    }
    Ok(values)
}

fn get_node(kb: &KnowledgeBase, args: &Value) -> Result<Value, String> {
    let names = required_strings(args, "names")?;
    let filters = NodeFilters {
        side: parse_side(args.get("side"))?,
        node_graph: optional_string(args, "node_graph")?,
    };
    let results = kb.get_node_scoped(&names, &filters);
    Ok(json!({ "results": results }))
}

fn list_docs(kb: &KnowledgeBase, args: &Value) -> Result<Value, String> {
    let keywords = strings(args, "keywords").unwrap_or_default();
    Ok(json!({ "results": kb.list_docs(&keywords) }))
}

fn read_doc(kb: &KnowledgeBase, args: &Value) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| "key 不能为空".to_owned())?;
    let from_anchor = args.get("from_anchor").and_then(Value::as_str);
    match kb.read_doc(key, from_anchor) {
        Some(doc) => Ok(json!(doc)),
        None => Err(format!("知识库中没有标题或 path_id 匹配「{key}」的文档")),
    }
}

fn search(kb: &KnowledgeBase, args: &Value) -> Result<Value, String> {
    let queries = required_strings(args, "queries")?;
    let top_k = top_k(args)?;
    let hits = kb.search(&queries, top_k);
    let results: Vec<Value> = queries
        .iter()
        .zip(hits)
        .map(|(query, hits)| json!({ "query": query, "hits": hits }))
        .collect();
    Ok(json!({ "results": results }))
}

fn rag_search(kb: &KnowledgeBase, args: &Value) -> Result<Value, String> {
    let queries = required_strings(args, "queries")?;
    let top_k = top_k(args)?;
    let hits = plan_rag_search(kb, &queries, top_k);
    let results: Vec<Value> = queries
        .iter()
        .zip(hits)
        .map(|(query, hits)| json!({ "query": query, "hits": hits }))
        .collect();
    Ok(json!({ "results": results }))
}

fn search_lua_api(kb: &KnowledgeBase, args: &Value) -> Result<Value, String> {
    let queries = required_strings(args, "queries")?;
    let top_k = top_k(args)?;
    let api_doc = kb
        .docs()
        .iter()
        .find(|doc| doc.title.contains("客户端") && doc.title.to_ascii_lowercase().contains("api"))
        .ok_or_else(|| {
            "本地目录中没有客户端 Lua API 文档；请先更新目录并镜像该篇文档".to_owned()
        })?;
    let filters = wonderland_knowledge::SearchFilters {
        doc_path_id: Some(api_doc.path_id.clone()),
        tables_only: true,
        ..Default::default()
    };
    let text_filters = wonderland_knowledge::SearchFilters {
        tables_only: false,
        ..filters.clone()
    };
    let results: Vec<Value> = queries
        .iter()
        .map(|query| {
            let table_hits = kb
                .search_scoped(std::slice::from_ref(query), top_k, &filters)
                .remove(0);
            let hits = if table_hits.is_empty() {
                kb.search_scoped(std::slice::from_ref(query), top_k, &text_filters)
                    .remove(0)
            } else {
                table_hits
            };
            json!({ "query": query, "hits": hits })
        })
        .collect();
    Ok(json!({ "source": api_doc.title, "results": results }))
}

fn top_k(args: &Value) -> Result<usize, String> {
    match args.get("top_k").and_then(Value::as_i64) {
        Some(value) if (1..=20).contains(&value) => Ok(value as usize),
        Some(value) => Err(format!("top_k 必须在 1–20 之间，收到 {value}")),
        None => Ok(5),
    }
}

fn optional_string(args: &Value, field: &str) -> Result<Option<String>, String> {
    match args.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| format!("{field} 必须是非空字符串")),
    }
}

fn parse_side(value: Option<&Value>) -> Result<Option<Side>, String> {
    match value.and_then(Value::as_str).map(str::trim) {
        None | Some("") => Ok(None),
        Some("server") => Ok(Some(Side::Server)),
        Some("client") => Ok(Some(Side::Client)),
        Some(other) => Err(format!("side 仅支持 server 或 client，收到 {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 极简夹具：只验工具的参数校验与返回结构，不重复 knowledge 的检索用例。
    fn empty_kb() -> KnowledgeBase {
        wonderland_knowledge::build_from_json("[]", &[]).unwrap()
    }

    #[test]
    fn tool_list_declares_schemas_and_read_only() {
        let tools = list();
        let items = tools["tools"].as_array().unwrap();
        let names: Vec<&str> = items
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "get_node",
                "list_docs",
                "read_doc",
                "search",
                "rag_search",
                "search_lua_api"
            ]
        );
        for tool in items {
            assert!(
                tool["inputSchema"].is_object(),
                "{} 缺 inputSchema",
                tool["name"]
            );
            assert!(
                tool["outputSchema"].is_object(),
                "{} 缺 outputSchema",
                tool["name"]
            );
            assert_eq!(tool["annotations"]["readOnlyHint"], json!(true));
            let description = tool["description"].as_str().unwrap();
            assert!(
                description.contains("千星") || description.contains("知识库"),
                "描述要写清资料来源：{}",
                tool["name"]
            );
        }
        let get_node = &items[0];
        let result_item = &get_node["outputSchema"]["properties"]["results"]["items"];
        assert!(
            result_item["properties"]["matches"].is_object(),
            "get_node 的 schema 必须与 NodeLookup 实际序列化字段一致"
        );
        assert!(result_item["properties"]["candidates"].is_null());
    }

    #[test]
    fn empty_required_list_is_a_readable_error() {
        let kb = empty_kb();
        let error = call(&kb, "get_node", &json!({ "names": [] })).unwrap_err();
        assert!(error.contains("names"));
        let error = call(&kb, "search", &json!({ "queries": [""] })).unwrap_err();
        assert!(error.contains("queries"));
    }

    #[test]
    fn top_k_out_of_range_is_rejected() {
        let kb = empty_kb();
        let error = call(&kb, "search", &json!({ "queries": ["x"], "top_k": 99 })).unwrap_err();
        assert!(error.contains("1–20"));
    }

    #[test]
    fn unknown_tool_and_missing_key_are_errors() {
        let kb = empty_kb();
        assert!(
            call(&kb, "nope", &json!({}))
                .unwrap_err()
                .contains("未知工具")
        );
        assert!(
            call(&kb, "read_doc", &json!({ "key": "  " }))
                .unwrap_err()
                .contains("key")
        );
    }

    #[test]
    fn successful_call_returns_text_and_structured_content() {
        let kb = empty_kb();
        let result = call(&kb, "list_docs", &json!({})).unwrap();
        assert_eq!(result["content"][0]["type"], json!("text"));
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("results")
        );
        assert!(result["structuredContent"]["results"].is_array());
    }
}
