//! 用本地缓存复核重建结果（不联网、不写盘）。
//!
//! 用法：`cargo run -p wonderland-knowledge --example rebuild -- [缓存目录，默认 target/knowledge-spike/mirror-cache]`
//!
//! 目录里应有 `bundle.json`（catalog）与 `pages/` + `mirror.json`（页面镜像）——
//! 用 `examples/mirror.rs` 或界面上的「镜像知识库」产出。没有镜像时重建出的是空索引。

use std::{env, path::PathBuf, process::ExitCode};

use wonderland_knowledge::cache::KnowledgeCache;
use wonderland_knowledge::mirror::MirrorStore;

fn main() -> ExitCode {
    let dir = PathBuf::from(
        env::args()
            .nth(1)
            .unwrap_or_else(|| "target/knowledge-spike/mirror-cache".into()),
    );
    let cache = KnowledgeCache::new(dir);
    let bundle = match cache.load() {
        Ok(Some(bundle)) => bundle,
        Ok(None) => {
            eprintln!("{} 里没有可用缓存（bundle.json）", cache.dir().display());
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("读缓存失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    let pages = match MirrorStore::new(cache.dir().to_path_buf()).load_pages() {
        Ok(pages) => pages,
        Err(error) => {
            eprintln!("读页面镜像失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("页面镜像 {} 篇", pages.len());

    let kb = match wonderland_knowledge::build_from_json(&bundle.catalog, &pages) {
        Ok(kb) => kb,
        Err(error) => {
            eprintln!("重建失败：{error}");
            return ExitCode::FAILURE;
        }
    };
    match serde_json::to_string_pretty(&kb.stats()) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("序列化统计失败：{error}");
            ExitCode::FAILURE
        }
    }
}
