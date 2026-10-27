use serde_json::{Value, json};
use wonderland_knowledge::{NodeFilters, SearchFilters, Side, workspace::Workspace};

pub fn list() -> Value {
    let strings = json!({"type":"array","items":{"type":"string","minLength":1,"maxLength":8192},"minItems":1,"maxItems":20});
    let libraries =
        json!({"type":"array","items":{"type":"string","maxLength":160},"maxItems":100});
    let specs = [
        (
            "list_libraries",
            "列出已显式暴露的知识库。新建库默认不暴露。",
            json!({}),
            vec![],
        ),
        (
            "list_docs",
            "列出已暴露库的文档及出处，可用 keywords 筛选标题。",
            json!({"library_ids":libraries,"keywords":strings,"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":100}}),
            vec![],
        ),
        (
            "read_doc",
            "按文档 ID 或完整标题读取原文块；同名文档整组返回并标明所属库。offset/limit 为正文字符分页。",
            json!({"library_ids":libraries,"key":{"type":"string","minLength":1,"maxLength":8192},"from_anchor":{"type":"string","maxLength":256},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":50000}}),
            vec!["key"],
        ),
        (
            "search",
            "检索已暴露库，结合关键词及已构建的本地语义索引，返回片段、库和出处。",
            json!({"library_ids":libraries,"queries":strings,"top_k":{"type":"integer","minimum":1,"maximum":20}}),
            vec!["queries"],
        ),
        (
            "rag_search",
            "开放问题、方案分析和排障检索；使用共同的混合召回及基于实际命中的补充检索。返回资料证据，不生成答案。",
            json!({"library_ids":libraries,"queries":strings,"top_k":{"type":"integer","minimum":1,"maximum":20}}),
            vec!["queries"],
        ),
        (
            "get_node",
            "千星预设扩展：按节点名查说明、端侧及参数表，同名候选整组返回。只访问已暴露的千星资料源。",
            json!({"library_ids":libraries,"names":strings,"side":{"enum":["server","client"]},"node_graph":{"type":"string","maxLength":256}}),
            vec!["names"],
        ),
        (
            "search_lua_api",
            "千星预设扩展：定向检索客户端 Lua API 文档中的函数与参数表。",
            json!({"library_ids":libraries,"queries":strings,"top_k":{"type":"integer","minimum":1,"maximum":20}}),
            vec!["queries"],
        ),
    ];
    json!({"tools":specs.into_iter().map(|(name,description,properties,required)|json!({"name":name,"description":description,
        "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
        "outputSchema":{"type":"object","properties":{"results":{"type":"array"},"warnings":{"type":"array","items":{"type":"string"}}},"required":["results","warnings"],"additionalProperties":false},
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
    })).collect::<Vec<_>>()})
}
fn strings(args: &Value, key: &str, required: bool) -> Result<Vec<String>, String> {
    let values = match args.get(key) {
        Some(value) => serde_json::from_value::<Vec<String>>(value.clone())
            .map_err(|_| format!("{key} 必须为字符串数组"))?,
        None => vec![],
    };
    if values.len() > 100
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 8192)
        || (required && values.is_empty())
    {
        return Err(format!("{key} 参数无效"));
    }
    Ok(values)
}
fn count(args: &Value, key: &str, default: usize, max: usize) -> Result<usize, String> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0 && *value <= max as u64)
            .map(|value| value as usize)
            .ok_or_else(|| format!("{key} 范围应为 1–{max}")),
    }
}

pub fn call(workspace: &Workspace, name: &str, args: &Value) -> Result<Value, String> {
    let schema = list();
    let spec = schema["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == name)
        .ok_or("未知工具")?;
    let object = args.as_object().ok_or("工具参数应为对象")?;
    if object
        .keys()
        .any(|key| spec["inputSchema"]["properties"].get(key).is_none())
    {
        return Err("包含不支持的参数".into());
    }
    let ids = strings(args, "library_ids", false)?;
    let libraries = workspace.scope(&ids, true)?;
    let mut results = Vec::new();
    let mut warnings = Vec::new();
    match name {
        "list_libraries" => {
            for library in &libraries {
                results.push(json!({"library_id":library.id,"name":library.name,"preset_id":library.preset_id}));
            }
        }
        "search" | "rag_search" => {
            let queries = strings(args, "queries", true)?;
            if queries.len() > 20 {
                return Err("每次最多 20 个查询".into());
            }
            let top = count(args, "top_k", 5, 20)?;
            for query in queries {
                let found = workspace.search(&ids, &query, top, true)?;
                warnings.extend(found.warnings);
                results.push(json!({"query":query,"hits":found.hits}));
            }
        }
        "list_docs" => {
            let keywords = strings(args, "keywords", false)?;
            for library in &libraries {
                let documents = workspace.documents(&library.id, true)?;
                for document in documents.as_array().ok_or("文档清单无效")? {
                    if keywords.is_empty()
                        || keywords.iter().any(|keyword| {
                            document["title"]
                                .as_str()
                                .unwrap_or_default()
                                .to_lowercase()
                                .contains(&keyword.to_lowercase())
                        })
                    {
                        results.push(document.clone());
                    }
                }
            }
            let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
            results = results
                .into_iter()
                .skip(offset)
                .take(count(args, "limit", 50, 100)?)
                .collect();
        }
        "read_doc" => {
            let key = args
                .get("key")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or("key 不能为空")?;
            let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
            let limit = count(args, "limit", 20000, 50000)?;
            for library in &libraries {
                let snapshot = workspace.snapshot(library)?;
                for document in snapshot
                    .documents
                    .iter()
                    .filter(|document| document.id == key || document.title == key)
                {
                    let kb = workspace.knowledge_base(library)?;
                    if let Some(mut text) = kb.read_doc(
                        &document.id,
                        args.get("from_anchor").and_then(Value::as_str),
                    ) {
                        let mut skip = offset;
                        let mut remaining = limit;
                        let mut truncated = false;
                        text.blocks.retain_mut(|block| {
                            let length = block.text.chars().count();
                            if skip >= length {
                                skip -= length;
                                return false;
                            }
                            let content = block
                                .text
                                .chars()
                                .skip(skip)
                                .take(remaining)
                                .collect::<String>();
                            skip = 0;
                            let used = content.chars().count();
                            truncated |= used < length;
                            remaining -= used;
                            block.text = content;
                            !block.text.is_empty()
                        });
                        results.push(json!({"library_id":library.id,"library_name":library.name,"document":text,"offset":offset,"next_offset":if truncated||remaining==0{Some(offset+limit)}else{None}}));
                    }
                }
            }
            if results.is_empty() {
                return Err("已暴露的知识库中未找到该文档，请先使用 list_docs 确认文档 ID".into());
            }
        }
        "get_node" | "search_lua_api" => {
            let values = strings(
                args,
                if name == "get_node" {
                    "names"
                } else {
                    "queries"
                },
                true,
            )?;
            let side = match args.get("side").and_then(Value::as_str) {
                None => None,
                Some("server") => Some(Side::Server),
                Some("client") => Some(Side::Client),
                _ => return Err("side 仅支持 server/client".into()),
            };
            let top = count(args, "top_k", 5, 20)?;
            for library in &libraries {
                if !library
                    .sources
                    .iter()
                    .any(|source| source.kind == "wonderland")
                {
                    continue;
                }
                let kb = workspace.knowledge_base(library)?;
                if name == "get_node" {
                    let found = kb.get_node_scoped(
                        &values,
                        &NodeFilters {
                            side,
                            node_graph: args
                                .get("node_graph")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        },
                    );
                    results.push(
                        json!({"library_id":library.id,"library_name":library.name,"nodes":found}),
                    );
                } else {
                    let api_docs = kb
                        .docs()
                        .iter()
                        .filter(|document| {
                            document.title.contains("客户端")
                                && document.title.to_ascii_lowercase().contains("api")
                        })
                        .collect::<Vec<_>>();
                    if api_docs.is_empty() {
                        warnings.push(format!("{} 尚未构建客户端 Lua API 文档", library.name));
                    }
                    for document in api_docs {
                        let hits = kb.search_scoped(
                            &values,
                            top,
                            &SearchFilters {
                                doc_path_id: Some(document.path_id.clone()),
                                ..SearchFilters::default()
                            },
                        );
                        results.push(json!({"library_id":library.id,"library_name":library.name,"queries":values,"hits":hits}));
                    }
                }
            }
            if results.is_empty() && warnings.is_empty() {
                return Err("请先在工作台构建千星预设资料并显式允许 MCP 访问".into());
            }
        }
        _ => return Err("未知工具".into()),
    }
    // Reads recheck authorization after disk/model work as well as before it.
    for library in &libraries {
        workspace.library(&library.id, true)?;
    }
    Ok(json!({"results":results,"warnings":warnings}))
}
