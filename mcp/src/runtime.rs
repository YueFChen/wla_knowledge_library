use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};
use wonderland_knowledge::workspace::Workspace;

pub fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut root = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cache-dir" => root = Some(PathBuf::from(args.next().ok_or("--cache-dir 缺少目录")?)),
            "--help" | "-h" => {
                eprintln!(
                    "wla_knowledge_mcp --cache-dir ABSOLUTE_DIRECTORY\n本地只读 MCP，权限由工作台的暴露清单决定。"
                );
                return Ok(());
            }
            _ => return Err(format!("未知参数：{arg}")),
        }
    }
    let root = root
        .filter(|path| path.is_absolute())
        .ok_or("请从知识库设置复制带绝对数据目录的 MCP 配置")?;
    let workspace = Workspace::new(root);
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        if line.len() > 1024 * 1024 {
            return Err("MCP 请求超过 1 MiB".into());
        }
        if let Some(response) = handle(&workspace, &line) {
            writeln!(stdout, "{response}")
                .and_then(|()| stdout.flush())
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn handle(workspace: &Workspace, line: &str) -> Option<Value> {
    let request: Value = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":error.to_string()}}),
            );
        }
    };
    let id = request.get("id")?.clone();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => {
            json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"wla_knowledge_library","version":env!("CARGO_PKG_VERSION")},"instructions":"本地只读多库知识库。默认检索全部已显式暴露的库，可用 library_ids 缩小范围。结果标明库、文档和出处，回答资料问题时引用来源；查询失败不代表资料不存在。先用 list_libraries 和 list_docs 了解范围，search 查明确概念，rag_search 查开放问题，read_doc 读原文。get_node 和 search_lua_api 只适用于已暴露的千星预设资料。"})
        }
        "ping" => json!({}),
        "tools/list" => super::tools::list(),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match super::tools::call(
                workspace,
                params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                &arguments,
            ) {
                Ok(value) => {
                    json!({"isError":false,"content":[{"type":"text","text":value.to_string()}],"structuredContent":value})
                }
                Err(error) => json!({"isError":true,"content":[{"type":"text","text":error}]}),
            }
        }
        _ => {
            return Some(
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"不支持的方法"}}),
            );
        }
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
