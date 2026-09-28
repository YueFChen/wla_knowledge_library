//! `mcp-knowledge`：知识库的 MCP 入口（headless 二进制）。
//!
//! 由外部 AI 客户端按 MCP 标准以 stdio 拉起（ADR-010）：**不监听端口、无常驻服务、不接触密钥**，
//! 只读地暴露结构化节点 / 文档读取、普通检索、专门的 `rag_search` 与 `search_lua_api` 工具。
//! 结构化数据与普通检索来自本插件内部的知识库领域 crate；复杂 RAG 的场景规划由
//! `wonderland-knowledge-library` 插件负责，GUI 与 MCP 共用同一份规划。
//!
//! **只读**（ADR-010「不把抓取放进 MCP 进程」）：不抓取、不渲染、不修改知识内容。启动时只读
//! `catalog.json` 与页面镜像；本地嵌入模型不在 MCP 启动时自动执行；内容源是镜像，缺目录或缺镜像都给出可读原因，
//! 工具调用按 MCP 约定返回 `isError`。
//!
//! 客户端配置示例（Claude Desktop / Cursor 等，需填**绝对路径**，Windows 下 GUI 不继承 PATH）：
//!
//! ```json
//! { "mcpServers": { "wonderland-knowledge": {
//!     "command": "C:\\path\\to\\mcp-knowledge.exe", "args": ["--cache-dir", "C:\\path\\to\\plugin-data\\knowledge_library"] } } }
//! ```
//!
//! 参数：
//! - `--lang <zh-cn>`：知识库语种，默认 `zh-cn`；
//! - `--cache-dir <DIR>`：指定缓存目录（默认 `<app_data>/plugin-data/knowledge_library`）；
//! - `--expose <guide[,guide]>`：按配置只暴露选中的知识库（当前只有 `guide`）。
//!
//! 诊断输出一律走 stderr——stdout 只允许出现协议消息（ADR-010）。

mod server;
mod tools;

use std::io::{self, BufReader, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use wonderland_knowledge::cache::KnowledgeCache;
use wonderland_knowledge::mirror::MirrorStore;
use wonderland_knowledge::{NO_MIRROR, build_from_json};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // 起不来就把原因写到 stderr：客户端会把这一行展示给用户。
            eprintln!("mcp-knowledge 启动失败：{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let options = Options::parse(std::env::args().skip(1))?;
    if options.help {
        eprintln!("{USAGE}");
        return Ok(());
    }

    let cache = match &options.cache_dir {
        Some(dir) => KnowledgeCache::new(dir.clone()),
        None => KnowledgeCache::for_plugin_data(),
    };
    eprintln!(
        "mcp-knowledge {}｜语种 {}｜缓存 {}｜只读（不抓取 / 不渲染 / 不修改知识内容）",
        env!("CARGO_PKG_VERSION"),
        options.lang,
        cache.dir().display()
    );

    // 缺目录 / 缺镜像都不退出进程：把可读原因写给客户端，工具调用照 MCP 约定回 isError。
    let server = match prepare(&cache) {
        Ok(kb) => {
            let stats = kb.stats();
            eprintln!(
                "知识库就绪：文档 {} 篇（叶子 {}，已镜像 {}）｜块 {}｜含表块 {}｜节点 {}",
                stats.docs,
                stats.leaf_docs,
                stats.mirrored_docs,
                stats.blocks,
                stats.tables,
                stats.nodes
            );
            server::Server::new(kb)
        }
        Err(message) => {
            eprintln!("知识库不可用：{message}（工具调用会返回 isError）");
            server::Server::unavailable(message)
        }
    };

    let stdin = BufReader::new(io::stdin().lock());
    let stdout = io::stdout().lock();
    server
        .serve(stdin, stdout)
        .map_err(|error| format!("stdio 读写失败：{error}"))?;
    let _ = io::stdout().flush();
    Ok(())
}

/// 只读地准备索引：目录与页面镜像都取自本地缓存，缺什么就给可读原因。
fn prepare(cache: &KnowledgeCache) -> Result<wonderland_knowledge::KnowledgeBase, String> {
    let bundle = cache
        .load()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "本地还没有知识库缓存，请先在应用里抓取知识库".to_owned())?;
    let pages = MirrorStore::new(cache.dir().to_path_buf())
        .load_pages()
        .map_err(|error| error.to_string())?;
    if pages.is_empty() {
        return Err(NO_MIRROR.to_owned());
    }
    let mut kb = build_from_json(&bundle.catalog, &pages).map_err(|error| error.to_string())?;
    // MCP may reuse a compatible prebuilt vector cache, but must never build one at startup.
    if let Err(error) = kb.enable_cached_local_embeddings(cache.dir()) {
        eprintln!("本地向量索引无法加载，继续使用关键词检索：{error}");
    }
    Ok(kb)
}

const USAGE: &str = "\
用法：mcp-knowledge [--lang zh-cn] [--cache-dir DIR] [--expose guide]

知识库的 MCP 入口：只读工具，stdio 传输，由 AI 客户端按 MCP 标准拉起。
只读：不抓取、不渲染、不修改知识内容——目录、页面镜像与向量索引由应用 GUI 准备好。
";

/// 命令行参数（手写解析：只有这几个开关，不值得引入参数库）。
struct Options {
    lang: String,
    cache_dir: Option<PathBuf>,
    help: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            lang: wonderland_knowledge::DEFAULT_LANG.to_owned(),
            cache_dir: None,
            help: false,
        };
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| -> Result<String, String> {
                args.next().ok_or_else(|| format!("{name} 缺少取值"))
            };
            match arg.as_str() {
                "--lang" => options.lang = value("--lang")?,
                "--cache-dir" => options.cache_dir = Some(PathBuf::from(value("--cache-dir")?)),
                "--expose" => {
                    let raw = value("--expose")?;
                    for name in raw
                        .split(',')
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    {
                        if name != "guide" {
                            return Err(format!("暂不支持的知识库：{name}（当前只有 guide）"));
                        }
                    }
                }
                "--help" | "-h" => options.help = true,
                other => return Err(format!("未知参数：{other}\n\n{USAGE}")),
            }
        }
        Ok(options)
    }
}
