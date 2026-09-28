use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use base64::Engine as _;
use serde_json::{Value, json};
use wonderland_knowledge::{DEFAULT_LANG, KNOWLEDGE_BASE_URL};
use wonderland_knowledge_library::{KnowledgeLibrary, MirrorProgress};
use wonderland_plugin_sdk::{HostClient, PluginError, PluginFailure, RequestTracker, serve};

const CONTRACT: &str = include_str!("../package/contract.json");

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
    if let Err(error) = run() {
        eprintln!("knowledge_library backend stopped: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?,
    );
    let library = Arc::new(std::sync::OnceLock::<KnowledgeLibrary>::new());
    let active_mirror = RequestTracker::default();

    serve(
        "knowledge_library",
        "0.2.0",
        CONTRACT,
        move |host, method, params, request_id| {
            dispatch(
                &host,
                &library,
                &runtime,
                &active_mirror,
                &method,
                params,
                request_id,
            )
            .map_err(|error| match error {
                DispatchError::Plugin(error) => PluginError::new(
                    format!(
                        "PLUGIN_KNOWLEDGE_LIBRARY_{}",
                        error.code().to_ascii_uppercase()
                    ),
                    error.to_string(),
                ),
                DispatchError::Core(error) => error,
            })
        },
    )
    .map_err(|error| error.to_string())
}

fn plugin_data_dir() -> Result<PathBuf, PluginFailure> {
    let path = std::env::var_os("WONDERLAND_PLUGIN_DATA_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| PluginFailure::LocalData("Core 未提供知识库插件的数据目录。".to_owned()))?;
    if !path.is_absolute() {
        return Err(PluginFailure::LocalData(
            "Core 提供的知识库插件数据目录不是绝对路径。".to_owned(),
        ));
    }
    Ok(path)
}

fn dispatch(
    host: &HostClient,
    library: &OnceLock<KnowledgeLibrary>,
    runtime: &tokio::runtime::Runtime,
    active_mirror: &RequestTracker,
    method: &str,
    params: Value,
    request_id: Option<String>,
) -> Result<Value, DispatchError> {
    if method == "__cancel" {
        if params
            .get("requestId")
            .and_then(Value::as_str)
            .is_some_and(|request_id| active_mirror.matches(request_id))
            && let Some(library) = library.get()
        {
            library.cancel_mirror();
        }
        return Ok(Value::Null);
    }
    let library = match library.get() {
        Some(library) => library,
        None => {
            let data_dir = plugin_data_dir()?;
            library.get_or_init(|| KnowledgeLibrary::new(data_dir))
        }
    };

    match method {
        "status" => value(Ok(library.status())),
        "embedding_status" => value(library.embedding_status()),
        "activate_embeddings" => value(library.activate_embeddings()),
        "disable_embeddings" => value(library.disable_embeddings()),
        "recheck_embeddings" => value(library.recheck_embeddings()),
        "refresh" => {
            let url = format!("{KNOWLEDGE_BASE_URL}/{DEFAULT_LANG}/catalog.json");
            let response = host
                .call_core(
                    "core.network.public",
                    json!({
                        "method": "GET", "url": url, "headers": [], "query": [], "body": null,
                    }),
                )
                .map_err(host_error)?;
            let encoded = response
                .get("contentBase64")
                .and_then(Value::as_str)
                .ok_or(PluginFailure::InvalidResponse)?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| PluginFailure::InvalidResponse)?;
            let catalog = String::from_utf8(bytes).map_err(|_| PluginFailure::InvalidResponse)?;
            value(library.refresh(catalog))
        }
        "search" => {
            let queries: Vec<String> = param(&params, "queries")?;
            let top_k = params
                .get("top_k")
                .and_then(Value::as_u64)
                .ok_or(PluginFailure::InvalidInput)? as u32;
            value(library.search_rag(queries, top_k))
        }
        "docs" => value(library.list_docs(param(&params, "keywords")?)),
        "open_source" => {
            let path_id = string_param(&params, "path_id")?;
            let _ = library.source_url(path_id)?;
            Ok(host
                .call_core(
                    "core.browser.open_official",
                    json!({ "target": "document", "pathId": path_id }),
                )
                .map_err(host_error)?)
        }
        "open_model_source" => {
            let target = match string_param(&params, "source")? {
                "onnx" => "onnx",
                "model" => "model",
                _ => return Err(PluginFailure::InvalidInput.into()),
            };
            Ok(host
                .call_core("core.browser.open_official", json!({ "target": target }))
                .map_err(host_error)?)
        }
        "reveal_cache_dir" => Ok(host
            .call_core("core.files.reveal_own", json!({ "scope": "data" }))
            .map_err(host_error)?),
        "mirror_status" => value(library.mirror_status()),
        "mirror_start" => {
            let request_id = request_id.as_deref().ok_or(PluginFailure::InvalidInput)?;
            let _active = active_mirror.begin(request_id).ok_or(PluginFailure::Busy)?;
            let limit = params
                .get("limit")
                .filter(|value| !value.is_null())
                .and_then(Value::as_u64)
                .map(|value| value as usize);
            let event_host = host.clone();
            let event_id = Some(request_id.to_owned());
            let run =
                runtime.block_on(library.mirror(limit, move |progress: MirrorProgress| {
                    if let Ok(payload) = serde_json::to_value(progress) {
                        let _ = event_host.emit("mirror.progress", event_id.as_deref(), payload);
                    }
                }))?;
            value(Ok(run))
        }
        "page" => value(library.page(string_param(&params, "path_id")?)),
        "mirror_page" => {
            value(runtime.block_on(library.mirror_page(string_param(&params, "path_id")?)))
        }
        _ => Err(PluginFailure::InvalidInput.into()),
    }
}

#[derive(Debug)]
enum DispatchError {
    Plugin(PluginFailure),
    Core(PluginError),
}

impl From<PluginFailure> for DispatchError {
    fn from(error: PluginFailure) -> Self {
        Self::Plugin(error)
    }
}

impl From<PluginError> for DispatchError {
    fn from(error: PluginError) -> Self {
        Self::Core(error)
    }
}

fn value<T: serde::Serialize>(result: Result<T, PluginFailure>) -> Result<Value, DispatchError> {
    Ok(serde_json::to_value(result?).map_err(|_| PluginFailure::InvalidResponse)?)
}
fn param<T: serde::de::DeserializeOwned>(params: &Value, key: &str) -> Result<T, PluginFailure> {
    serde_json::from_value(
        params
            .get(key)
            .cloned()
            .ok_or(PluginFailure::InvalidInput)?,
    )
    .map_err(|_| PluginFailure::InvalidInput)
}
fn string_param<'a>(params: &'a Value, key: &str) -> Result<&'a str, PluginFailure> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or(PluginFailure::InvalidInput)
}
fn host_error(error: PluginError) -> PluginFailure {
    match error.code.as_str() {
        "TIMEOUT" => PluginFailure::Timeout,
        "UNAUTHORIZED" => PluginFailure::InvalidInput,
        "INVALID_INPUT" => PluginFailure::InvalidInput,
        _ => PluginFailure::Transport(error.message),
    }
}
