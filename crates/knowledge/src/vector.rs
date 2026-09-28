//! Optional local dense retrieval. Model assets are provisioned locally; this module never downloads.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fastembed::{
    InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};
use serde::{Deserialize, Serialize};

use crate::model::Block;
use tracing::{info, warn};
use wonderland_plugin_sdk::PluginFailure;

/// Stable model identity included in the vector-cache metadata.
pub const LOCAL_EMBEDDING_MODEL_ID: &str = "BAAI/bge-small-zh-v1.5@qdrant-onnx-v1";
pub const LOCAL_EMBEDDING_MODEL_DIR: &str = "bge-small-zh-v1.5";
const INDEX_SCHEMA: u32 = 1;
const BATCH_SIZE: usize = 32;
const VECTOR_DIMENSIONS: usize = 512;

/// Expected files under `<knowledge-cache>/models/bge-small-zh-v1.5/`.
pub const LOCAL_EMBEDDING_MODEL_FILES: &[&str] = &[
    "model_optimized.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

pub(crate) struct LocalVectorIndex {
    engine: Mutex<TextEmbedding>,
    vectors: Vec<Vec<f32>>,
}

#[derive(Serialize, Deserialize)]
struct VectorCache {
    schema: u32,
    model: String,
    corpus_fingerprint: u64,
    vectors: Vec<Vec<f32>>,
}

impl LocalVectorIndex {
    pub(crate) fn load_cached(
        blocks: &[Block],
        cache_dir: &Path,
    ) -> Result<Option<Self>, PluginFailure> {
        let model_paths = model_paths(cache_dir);
        if model_paths.iter().any(|path| !path.is_file()) {
            return Ok(None);
        }
        let Some(vectors) = cached_vectors(blocks, cache_dir) else {
            return Ok(None);
        };
        let engine = load_engine(&model_paths)?;
        info!(vectors = vectors.len(), "兼容的本地向量索引已载入");
        Ok(Some(Self {
            engine: Mutex::new(engine),
            vectors,
        }))
    }

    pub(crate) fn load_or_build(
        blocks: &[Block],
        cache_dir: &Path,
        allow_build: bool,
    ) -> Result<Self, PluginFailure> {
        let model_paths = model_paths(cache_dir);
        if model_paths.iter().any(|path| !path.is_file()) {
            let model_dir = cache_dir.join("models").join(LOCAL_EMBEDDING_MODEL_DIR);
            return Err(PluginFailure::LocalData(format!(
                "本地嵌入模型未安装；请将 BGE-small-zh-v1.5 ONNX 与 tokenizer 文件放入 {}",
                model_dir.display()
            )));
        }

        let index_path = index_path(cache_dir);
        if let Some(vectors) = cached_vectors(blocks, cache_dir) {
            let engine = load_engine(&model_paths)?;
            info!(vectors = vectors.len(), "本地向量索引已载入");
            return Ok(Self {
                engine: Mutex::new(engine),
                vectors,
            });
        }
        if !allow_build {
            return Err(PluginFailure::LocalData(
                "MCP 为只读模式，本地向量缓存不存在或已过期".to_owned(),
            ));
        }

        let mut engine = load_engine(&model_paths)?;
        let texts: Vec<String> = blocks.iter().map(embed_text).collect();
        let corpus_fingerprint = fingerprint(blocks);
        let vectors = engine
            .embed(&texts, Some(BATCH_SIZE))
            .map_err(|error| PluginFailure::LocalData(format!("本地文档向量化失败：{error}")))?;
        let cache = VectorCache {
            schema: INDEX_SCHEMA,
            model: LOCAL_EMBEDDING_MODEL_ID.to_owned(),
            corpus_fingerprint,
            vectors: vectors.clone(),
        };
        if let Err(error) = store_atomic(&index_path, &cache) {
            warn!(reason = %error, "向量索引缓存写入失败，将在下次重建");
        } else {
            info!(vectors = vectors.len(), "本地向量索引已构建");
        }
        Ok(Self {
            engine: Mutex::new(engine),
            vectors,
        })
    }

    pub(crate) fn similarity_scores(&self, query: &str) -> Option<Vec<f32>> {
        let mut engine = self.engine.lock().ok()?;
        // BGE's recommended retrieval instruction belongs on queries; document text stays natural.
        let instructed = format!("为这个句子生成表示以用于检索相关文章：{query}");
        let mut vectors = engine.embed(&[instructed], Some(1)).ok()?;
        let query_vector = vectors.pop()?;
        Some(
            self.vectors
                .iter()
                .map(|vector| cosine(&query_vector, vector))
                .collect(),
        )
    }
}

fn model_paths(cache_dir: &Path) -> Vec<PathBuf> {
    let model_dir = cache_dir.join("models").join(LOCAL_EMBEDDING_MODEL_DIR);
    LOCAL_EMBEDDING_MODEL_FILES
        .iter()
        .map(|name| model_dir.join(name))
        .collect()
}

fn index_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("vectors-bge-small-zh-v1.5.json")
}

fn cached_vectors(blocks: &[Block], cache_dir: &Path) -> Option<Vec<Vec<f32>>> {
    let bytes = fs::read(index_path(cache_dir)).ok()?;
    let cache = serde_json::from_slice::<VectorCache>(&bytes).ok()?;
    (cache.schema == INDEX_SCHEMA
        && cache.model == LOCAL_EMBEDDING_MODEL_ID
        && cache.corpus_fingerprint == fingerprint(blocks)
        && cache.vectors.len() == blocks.len()
        && cache
            .vectors
            .iter()
            .all(|vector| vector.len() == VECTOR_DIMENSIONS))
    .then_some(cache.vectors)
}

fn load_engine(model_paths: &[PathBuf]) -> Result<TextEmbedding, PluginFailure> {
    let read = |index: usize| {
        fs::read(&model_paths[index])
            .map_err(|error| PluginFailure::LocalData(format!("读取本地嵌入模型失败：{error}")))
    };
    let onnx = read(0)?;
    let tokenizer = TokenizerFiles {
        tokenizer_file: read(1)?,
        config_file: read(2)?,
        special_tokens_map_file: read(3)?,
        tokenizer_config_file: read(4)?,
    };
    let model = UserDefinedEmbeddingModel::new(onnx, tokenizer).with_pooling(Pooling::Mean);
    TextEmbedding::try_new_from_user_defined(model, InitOptionsUserDefined::default())
        .map_err(|error| PluginFailure::LocalData(format!("初始化本地嵌入模型失败：{error}")))
}

fn embed_text(block: &Block) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        block.breadcrumb.join(" / "),
        block.heading_path.join(" / "),
        block.doc_title,
        block.text
    )
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let (mut dot, mut left_norm, mut right_norm) = (0.0f32, 0.0f32, 0.0f32);
    for (a, b) in left.iter().zip(right) {
        dot += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    let denominator = left_norm.sqrt() * right_norm.sqrt();
    if denominator == 0.0 {
        0.0
    } else {
        dot / denominator
    }
}

fn fingerprint(blocks: &[Block]) -> u64 {
    // FNV-1a is deliberately used instead of DefaultHasher so cache identity is stable across runs.
    let mut hash = 0xcbf29ce484222325u64;
    for block in blocks {
        for byte in embed_text(block).bytes().chain([0xff]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn store_atomic(path: &Path, cache: &VectorCache) -> Result<(), String> {
    let bytes = serde_json::to_vec(cache).map_err(|error| error.to_string())?;
    let temp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error.to_string())
        }
    }
}
