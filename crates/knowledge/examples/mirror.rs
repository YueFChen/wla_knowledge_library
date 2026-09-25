//! 把页面镜像到指定缓存目录（核查与"先跑一小批"用）。
//!
//! ```powershell
//! # 先由桌面插件刷新 catalog.json，再逐篇渲染详情页
//! cargo run -q -p wonderland-knowledge --example mirror -- target/knowledge-spike/mirror-cache 3
//! ```
//!
//! 参数：`<缓存目录> [最多镜像篇数]`；省略篇数即以默认 4 路并发镜像当前目录中的全部文档。
use std::path::PathBuf;

use wonderland_knowledge::build_from_json;
use wonderland_knowledge::cache::KnowledgeCache;
use wonderland_knowledge::mirror::{MirrorOptions, MirrorStore, mirror_all};
use wonderland_knowledge::mirror_one;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("用法：mirror <缓存目录> [最多篇数]"));
    let limit = args.next().and_then(|value| value.parse::<usize>().ok());

    let cache = KnowledgeCache::new(dir);
    let bundle = cache.load().expect("读取 catalog 缓存").expect(
        "没有 catalog 缓存；请先从桌面插件刷新知识库目录",
    );
    // 只从 catalog 取文档树：镜像作业要把**全部**目录节点列出来（含还没镜像的）。
    let kb = build_from_json(&bundle.catalog, &[]).expect("catalog 应可解析");
    let stats = kb.stats();
    println!("知识库目录：{} 篇（叶子 {}）", stats.docs, stats.leaf_docs);

    let store = MirrorStore::new(cache.dir().to_path_buf());
    let options = MirrorOptions {
        limit,
        ..Default::default()
    };
    let (run, index) = mirror_all(
        &store,
        kb.docs(),
        options,
        |progress| {
            let mark = if progress.skipped {
                "跳过（已是新的）".to_owned()
            } else if let Some(failed) = &progress.failed {
                format!("失败：{failed}")
            } else {
                format!(
                    "{} 元素 · {} 表 · {} 图",
                    progress.elements, progress.tables, progress.images
                )
            };
            println!(
                "[{}/{}] {} — {mark}",
                progress.done, progress.total, progress.title
            );
        },
        || false,
    )
    .await
    .expect("镜像作业失败");

    println!(
        "\n本次：镜像 {} · 跳过 {} · 失败 {}；索引共 {} 篇 / {} 元素 / {} 图（目录 {}）",
        run.mirrored,
        run.skipped,
        run.failed,
        index.pages(),
        index.elements(),
        index.images(),
        store.dir().display()
    );

    // 顺带验证"读一篇"的路径与单篇镜像一致（单篇镜像命令走的是同一个函数）。
    if let Some(doc) = kb.docs().iter().find(|doc| doc.is_leaf) {
        match mirror_one(&store, doc, 15_000).await {
            Ok(page) => println!(
                "单篇复核：{} 共 {} 个元素（首个标题：{:?}）",
                page.title,
                page.elements.len(),
                page.elements.first().and_then(|element| element.heading())
            ),
            Err(error) => println!("单篇复核失败：{error}"),
        }
    }
}
