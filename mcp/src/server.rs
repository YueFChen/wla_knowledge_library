//! stdio 上的 MCP 服务端（ADR-010：stdio 为主传输，由外部 AI 客户端 spawn 子进程）。
//!
//! 约定：
//! - **stdout 只允许出现协议消息**，任何诊断输出都走 stderr（ADR-010 的硬约束）；
//! - 只实现桌面场景需要的最小方法集：`initialize` / `ping` / `tools/list` / `tools/call`，
//!   通知（无 `id`）一律不回复；
//! - 工具执行失败按 MCP 约定放进结果里（`isError: true`），而不是协议级错误，便于模型自我纠正。

use std::io::{BufRead, Write};

use serde_json::{Value, json};
use wonderland_knowledge::KnowledgeBase;

use crate::tools;

/// 声明的协议版本：客户端请求同版本则原样回，否则回我们支持的这一版。
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// 服务端标识（出现在 `initialize` 的结果里，与工具名前缀无关）。
pub const SERVER_NAME: &str = "wonderland-knowledge";

/// 给支持 MCP `instructions` 的客户端提供领域边界和调用准则。
pub const SERVER_INSTRUCTIONS: &str = "千星奇域官方教程的本地只读知识库。用户询问节点名和参数时用 get_node；询问客户端 Lua UI 脚本 API 的接口、函数、类型、参数和返回值时用 search_lua_api；文档名明确时用 read_doc；直接查名称或概念时用 search；开放问题、方案分析和排障时用 rag_search。未命中可换词重试；工具失败表示查询失败，不表示官方资料不存在。";

pub struct Server {
    /// 索引；目录或镜像缺失时为 `None`，此时只有 `reason` 可答。
    kb: Option<KnowledgeBase>,
    /// 索引不可用的可读原因（工具调用按 MCP 约定把它作为 `isError` 返回）。
    reason: Option<String>,
}

impl Server {
    pub fn new(kb: KnowledgeBase) -> Self {
        Self {
            kb: Some(kb),
            reason: None,
        }
    }

    /// 索引不可用（缺目录 / 缺镜像）：进程照常服务，工具调用回可读错误。
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            kb: None,
            reason: Some(reason.into()),
        }
    }

    /// 处理一行输入，返回需要写回的一行（通知返回 `None`）。
    pub fn handle_line(&self, line: &str) -> Option<Value> {
        let request: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                return Some(error_response(
                    Value::Null,
                    -32700,
                    &format!("JSON 解析失败：{error}"),
                ));
            }
        };
        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();

        // 通知（无 id）：不回复，即便方法不认识。
        let id = id?;

        match method {
            "initialize" => Some(success(
                id,
                json!({
                    // 客户端请求同版本时原样回；请求别的版本也回我们自己支持的最高版本，
                    // 由客户端决定是否断开（规范行为）。
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
                    "instructions": SERVER_INSTRUCTIONS
                }),
            )),
            "ping" => Some(success(id, json!({}))),
            "tools/list" => Some(success(id, tools::list())),
            "tools/call" => Some(success(id, self.call_tool(&request))),
            other => Some(error_response(
                id,
                -32601,
                &format!("不支持的方法：{other}"),
            )),
        }
    }

    fn call_tool(&self, request: &Value) -> Value {
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return tool_error("tools/call 缺少 name");
        };
        let Some(kb) = self.kb.as_ref() else {
            // 索引不可用：原因照原样给模型，避免它把"没准备好"当成"知识库里没有"。
            return tool_error(
                self.reason
                    .as_deref()
                    .unwrap_or("知识库索引不可用，请先在应用里抓取目录并镜像页面"),
            );
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        match tools::call(kb, name, &arguments) {
            Ok(mut result) => {
                if let Some(object) = result.as_object_mut() {
                    object.insert("isError".to_owned(), json!(false));
                }
                result
            }
            Err(message) => tool_error(&message),
        }
    }

    /// 逐行读请求、逐行写响应（换行分隔的 JSON-RPC；客户端关闭 stdin 即退出）。
    pub fn serve(&self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(response) = self.handle_line(&line) {
                writeln!(output, "{response}")?;
                output.flush()?;
            }
        }
        Ok(())
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server::new(wonderland_knowledge::build_from_json("[]", &[]).unwrap())
    }

    #[test]
    fn unavailable_index_answers_tools_with_a_readable_error() {
        let server = Server::unavailable(wonderland_knowledge::NO_MIRROR);
        let called = server
            .handle_line(r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"search","arguments":{"queries":["建立字典"]}}}"#)
            .unwrap();
        assert_eq!(
            called["result"]["isError"],
            json!(true),
            "缺镜像时工具调用要回 isError，而不是协约级错误"
        );
        let text = called["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("镜像"), "原因要可读：{text}");
        // 工具清单与握手不依赖索引，照常可用。
        let listed = server
            .handle_line(r#"{"jsonrpc":"2.0","id":10,"method":"tools/list"}"#)
            .unwrap();
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn initialize_reports_protocol_and_tools_capability() {
        let response = server().handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}}"#,
        ).unwrap();
        assert_eq!(response["id"], json!(1));
        assert_eq!(
            response["result"]["protocolVersion"],
            json!(PROTOCOL_VERSION)
        );
        assert!(response["result"]["capabilities"]["tools"].is_object());
        assert_eq!(response["result"]["serverInfo"]["name"], json!(SERVER_NAME));
        assert_eq!(
            response["result"]["instructions"],
            json!(SERVER_INSTRUCTIONS)
        );
    }

    #[test]
    fn notifications_get_no_reply() {
        let server = server();
        assert!(
            server
                .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
        assert!(
            server
                .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#)
                .is_none()
        );
    }

    #[test]
    fn tools_list_and_call_round_trip() {
        let server = server();
        let listed = server
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .unwrap();
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 4);

        let called = server
            .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search","arguments":{"queries":["建立字典"]}}}"#)
            .unwrap();
        assert_eq!(called["result"]["isError"], json!(false));
        assert_eq!(called["result"]["content"][0]["type"], json!("text"));
    }

    #[test]
    fn tool_failure_is_reported_inside_the_result() {
        let server = server();
        let called = server
            .handle_line(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search","arguments":{"queries":[]}}}"#)
            .unwrap();
        assert_eq!(
            called["result"]["isError"],
            json!(true),
            "工具错误不能变成协议级错误"
        );
        assert!(
            called["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("queries")
        );
    }

    #[test]
    fn unknown_method_and_bad_json_are_protocol_errors() {
        let server = server();
        let unknown = server
            .handle_line(r#"{"jsonrpc":"2.0","id":5,"method":"resources/list"}"#)
            .unwrap();
        assert_eq!(unknown["error"]["code"], json!(-32601));

        let broken = server.handle_line("{not json").unwrap();
        assert_eq!(broken["error"]["code"], json!(-32700));
        assert_eq!(broken["id"], Value::Null);
    }

    #[test]
    fn serve_reads_lines_and_writes_one_response_each() {
        let server = server();
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            "\n"
        );
        let mut output = Vec::new();
        server.serve(input.as_bytes(), &mut output).unwrap();
        let lines: Vec<&str> = std::str::from_utf8(&output)
            .unwrap()
            .trim()
            .lines()
            .collect();
        assert_eq!(lines.len(), 2, "通知不应产生回复：{lines:?}");
        assert!(lines[0].contains("\"id\":1"));
        assert!(lines[1].contains("\"id\":2"));
    }
}
