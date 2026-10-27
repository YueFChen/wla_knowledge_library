use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, atomic::AtomicBool},
};
use wonderland_knowledge::{
    models, render,
    workspace::{Fetcher, Preset, Resource, Source, Workspace},
};
use wonderland_plugin_sdk::{
    HostClient, HttpHeader, HttpRequestParams, PluginError, RequestTracker, serve,
};

const CONTRACT: &str = include_str!("../package/contract.json");
pub fn run() -> Result<(), String> {
    let workspace = Arc::new(OnceLock::<Workspace>::new());
    let tracker = RequestTracker::default();
    serve(
        "wla_knowledge_library",
        env!("CARGO_PKG_VERSION"),
        CONTRACT,
        move |host, method, params, request_id| {
            let workspace = match workspace.get() {
                Some(workspace) => workspace,
                None => {
                    let path = std::env::var_os("WONDERLAND_PLUGIN_DATA_DIR")
                        .map(PathBuf::from)
                        .filter(|path| path.is_absolute())
                        .ok_or_else(|| {
                            PluginError::new("INVALID_DATA_DIR", "Core 未提供绝对数据目录")
                        })?;
                    workspace.get_or_init(|| Workspace::new(path))
                }
            };
            if method == "__cancel" {
                if params
                    .get("requestId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| tracker.matches(id))
                {
                    workspace.cancel();
                }
                return Ok(Value::Null);
            }
            let _tracked = if matches!(method.as_str(), "build") {
                let request_id = request_id
                    .as_deref()
                    .ok_or_else(|| PluginError::new("INVALID_INPUT", "缺少任务身份"))?;
                Some(
                    tracker
                        .begin(request_id)
                        .ok_or_else(|| PluginError::new("BUSY", "已有任务正在执行"))?,
                )
            } else {
                None
            };
            dispatch(&host, workspace, &method, &params)
                .map_err(|error| PluginError::new("KNOWLEDGE_ERROR", error))
        },
    )
    .map_err(|error| error.to_string())
}

fn string<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("缺少参数 {key}"))
}
fn decoded<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}
fn value<T: serde::Serialize>(result: Result<T, String>) -> Result<Value, String> {
    serde_json::to_value(result?).map_err(|error| error.to_string())
}

fn dispatch(
    host: &HostClient,
    workspace: &Workspace,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    match method {
        "status" => {
            let registry = workspace.registry()?;
            let mut presets = wonderland_knowledge::workspace::builtins();
            presets.extend(registry.presets);
            let binary = std::env::current_exe()
                .map_err(|error| error.to_string())?
                .with_file_name("wla_knowledge_mcp.exe");
            Ok(
                json!({"libraries":registry.libraries,"presets":presets,"models":registry.models,"services":registry.services,"job":workspace.job(),
                "cache_dir":workspace.root(),"edge_available":render::available(),"version":env!("CARGO_PKG_VERSION"),
                "mcp_config":{"mcpServers":{"wla_knowledge_library":{"command":binary,"args":["--cache-dir",workspace.root()]}}}}),
            )
        }
        "library_create" => {
            value(workspace.create(string(params, "name")?, string(params, "preset_id")?))
        }
        "library_update" => {
            let model = params
                .get("model")
                .map(|value| {
                    if value.is_null() {
                        Ok(None)
                    } else {
                        value
                            .as_str()
                            .map(|value| Some(value.into()))
                            .ok_or("模型参数无效".to_string())
                    }
                })
                .transpose()?;
            value(workspace.update(
                string(params, "library_id")?,
                params.get("name").and_then(Value::as_str),
                params.get("exposed").and_then(Value::as_bool),
                model,
            ))
        }
        "source_save" => value(workspace.save_source(
            string(params, "library_id")?,
            decoded::<Source>(params.get("source").ok_or("缺少资料源")?)?,
        )),
        "delete" => {
            workspace.delete(
                string(params, "library_id")?,
                params.get("source_id").and_then(Value::as_str),
                params.get("confirmed").and_then(Value::as_bool) == Some(true),
            )?;
            Ok(json!({"ok":true}))
        }
        "pick_paths" => {
            let directory = string(params, "kind")? == "directory";
            if !directory && string(params, "kind")? != "files" {
                return Err("选择类型无效".into());
            }
            let extensions = params
                .get("extensions")
                .map(decoded::<Vec<String>>)
                .transpose()?
                .unwrap_or_default();
            let references = extensions.iter().map(String::as_str).collect::<Vec<_>>();
            value(host.pick_paths(directory, &references).map_err(host_error))
        }
        "build" => {
            let _activity = host.begin_activity("构建知识库").map_err(host_error)?;
            value(
                workspace.build_with(
                    string(params, "library_id")?,
                    &HostFetcher(host.clone()),
                    params
                        .get("semantic")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    &HostModels(host.clone()),
                ),
            )
        }
        "job_cancel" => {
            workspace.cancel();
            Ok(json!({"ok":true}))
        }
        "search" => {
            let ids: Vec<String> = decoded(params.get("library_ids").ok_or("请指定知识库")?)?;
            if ids.is_empty() {
                return Err("请先选择要检索的知识库".into());
            }
            value(workspace.search_with(
                &ids,
                string(params, "query")?,
                params.get("top_k").and_then(Value::as_u64).unwrap_or(20) as usize,
                false,
                &HostModels(host.clone()),
            ))
        }
        "documents" => workspace.documents(string(params, "library_id")?, false),
        "document" => workspace.document_page(
            string(params, "library_id")?,
            string(params, "document_id")?,
            params.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize,
            params.get("anchor").and_then(Value::as_str),
        ),
        "asset" => {
            let bytes =
                workspace.asset(string(params, "library_id")?, string(params, "key")?, false)?;
            let mime =
                wonderland_knowledge::workspace::image_mime(&bytes).ok_or("不是受支持的图片")?;
            Ok(json!({"mime":mime,"base64":STANDARD.encode(bytes)}))
        }
        "document_export" => {
            let document = workspace.document(
                string(params, "library_id")?,
                string(params, "document_id")?,
                false,
            )?;
            let bytes =
                workspace.asset(string(params, "library_id")?, &document.raw_asset, false)?;
            value(
                host.export_bytes(
                    &format!("document-{}.txt", wonderland_knowledge::workspace::unique()),
                    &bytes,
                )
                .map_err(host_error),
            )
        }
        "library_export" => {
            let _activity = host.begin_activity("导出知识库").map_err(host_error)?;
            let path = workspace.export_library(string(params, "library_id")?)?;
            let result = export_file(
                host,
                &path,
                &format!(
                    "knowledge-library-{}.zip",
                    wonderland_knowledge::workspace::unique()
                ),
            );
            let _ = fs::remove_file(path);
            result
        }
        "library_import" => {
            let paths = host.pick_paths(false, &["zip"]).map_err(host_error)?;
            if paths.cancelled {
                return Ok(json!({"cancelled":true}));
            }
            if paths.paths.len() != 1 {
                return Err("请选择一个知识库备份".into());
            }
            let _activity = host.begin_activity("导入知识库").map_err(host_error)?;
            value(workspace.import_library(Path::new(&paths.paths[0])))
        }
        "preset_save" => {
            let library = workspace.library(string(params, "library_id")?, false)?;
            value(workspace.save_preset(Preset {
                id: String::new(),
                name: string(params, "name")?.into(),
                sources: library.sources,
            }))
        }
        "preset_export" => {
            let id = string(params, "preset_id")?;
            let preset = wonderland_knowledge::workspace::builtins()
                .into_iter()
                .chain(workspace.registry()?.presets)
                .find(|preset| preset.id == id)
                .ok_or("预设不存在")?;
            let bytes = serde_json::to_vec_pretty(&preset).map_err(|error| error.to_string())?;
            value(
                host.export_bytes(
                    &format!(
                        "knowledge-preset-{}.json",
                        wonderland_knowledge::workspace::unique()
                    ),
                    &bytes,
                )
                .map_err(host_error),
            )
        }
        "preset_import" => {
            let paths = host.pick_paths(false, &["json"]).map_err(host_error)?;
            if paths.cancelled {
                return Ok(json!({"cancelled":true}));
            }
            if paths.paths.len() != 1 {
                return Err("请选择一个预设文件".into());
            }
            let file = fs::File::open(&paths.paths[0]).map_err(|error| error.to_string())?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.len() > 1024 * 1024 {
                return Err("预设超过 1 MiB".into());
            }
            value(
                workspace.save_preset(
                    serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
                ),
            )
        }
        "task_logs" => workspace.task_logs(
            params
                .get("job_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            params.get("cursor").and_then(Value::as_u64).unwrap_or(0) as usize,
        ),
        "service_save" => {
            let mut service: models::Service =
                decoded(params.get("service").ok_or("缺少服务配置")?)?;
            let existing = if service.id.is_empty() {
                None
            } else {
                Some(workspace.service(&service.id)?)
            };
            if service.id.is_empty() {
                service.id = wonderland_knowledge::workspace::unique();
            }
            service.secret_id = existing.as_ref().and_then(|old| old.secret_id.clone());
            if params.get("clear_key").and_then(Value::as_bool) == Some(true) {
                service.secret_id = None;
            }
            if let Some(key) = params
                .get("api_key")
                .and_then(Value::as_str)
                .filter(|key| !key.is_empty())
            {
                if key.len() > 8192 {
                    return Err("认证信息过长".into());
                }
                service.secret_id = Some(format!("embedding_{}", service.id));
            }
            service.validate()?;
            if workspace.job().state == "running" {
                return Err("请等待当前任务完成".into());
            }
            if let Some(key) = params
                .get("api_key")
                .and_then(Value::as_str)
                .filter(|key| !key.is_empty())
            {
                host.call_core(
                    "core.secrets.plugin.set",
                    json!({"key":service.secret_id,"value":key}),
                )
                .map_err(host_error)?;
            } else if params.get("clear_key").and_then(Value::as_bool) == Some(true) {
                host.call_core(
                    "core.secrets.plugin.delete",
                    json!({"key":format!("embedding_{}",service.id)}),
                )
                .map_err(host_error)?;
            }
            value(workspace.save_service(service))
        }
        "service_models" => {
            let service = workspace.service(string(params, "service_id")?)?;
            value(models::list_models(
                &service,
                &HostModels(host.clone()),
                &AtomicBool::new(false),
            ))
        }
        "service_delete" => {
            let id = string(params, "service_id")?;
            workspace.remove_service(
                id,
                params.get("confirmed").and_then(Value::as_bool) == Some(true),
            )?;
            host.call_core(
                "core.secrets.plugin.delete",
                json!({"key":format!("embedding_{id}")}),
            )
            .map_err(host_error)?;
            Ok(json!({"ok":true}))
        }
        "model_test" | "model_save" => {
            let profile: models::ModelProfile =
                decoded(params.get("profile").ok_or("缺少模型配置")?)?;
            let texts = params
                .get("texts")
                .map(decoded::<Vec<String>>)
                .transpose()?
                .unwrap_or_else(|| {
                    vec![
                        "取消任务后保留最近成功的资料快照。".into(),
                        "Cancelling a task preserves the last successful document snapshot.".into(),
                        "The interface supports multiple independent libraries.".into(),
                    ]
                });
            let result = workspace.validate_model(profile, &texts, &HostModels(host.clone()))?;
            if method == "model_save" {
                value(workspace.save_model(decoded(&result["profile"])?))
            } else {
                Ok(result)
            }
        }
        "model_delete" => {
            workspace.remove_model(
                string(params, "model_id")?,
                params.get("confirmed").and_then(Value::as_bool) == Some(true),
            )?;
            Ok(json!({"ok":true}))
        }
        "open_url" => {
            let url = string(params, "url")?;
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err("仅支持打开 HTTP(S) 原文链接".into());
            }
            host.call_core("core.browser.open", json!({"url":url}))
                .map_err(host_error)
        }
        _ => Err("不支持的知识库方法".into()),
    }
}

struct HostModels(HostClient);
impl models::Transport for HostModels {
    fn request(
        &self,
        service: &models::Service,
        method: &str,
        url: &str,
        body: Option<&Value>,
        cancelled: &AtomicBool,
    ) -> Result<Value, String> {
        use wonderland_plugin_sdk::{HttpBody, SecretHeader};
        service.validate()?;
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err("任务已取消".into());
        }
        let mut request = HttpRequestParams::get(url);
        request.method = method.into();
        request.timeout_ms = Some(90000);
        if let Some(body) = body {
            request
                .headers
                .push(HttpHeader::text("Content-Type", "application/json"));
            request.body = Some(HttpBody::Text {
                content: body.to_string(),
            });
        }
        request.authentication = service.secret_id.as_ref().map(|key| SecretHeader {
            secret_id: key.clone(),
            name: "Authorization".into(),
            prefix: "Bearer ".into(),
        });
        let response = self
            .0
            .http_request(&request)
            .map_err(|_| "本机模型连接失败，请检查服务及认证".to_string())?;
        if !(200..300).contains(&response.status) {
            let _ = self.0.stream_close(&response.body);
            return Err(format!("模型服务 HTTP {}", response.status));
        }
        let mut output = Limited {
            inner: Vec::new(),
            remaining: 32 * 1024 * 1024,
        };
        self.0
            .copy_stream_to(&response.body, &mut output)
            .map_err(|_| "模型响应读取失败".to_string())?;
        serde_json::from_slice(&output.inner).map_err(|_| "模型未返回有效 JSON".into())
    }
}

struct HostFetcher(HostClient);
impl Fetcher for HostFetcher {
    fn get(&self, url: &str) -> Result<Resource, String> {
        let mut request = HttpRequestParams::get(url);
        request.timeout_ms = Some(30000);
        request.headers = vec![
            HttpHeader::text("User-Agent", "Wonderland-Knowledge/0.3"),
            HttpHeader::text("Accept-Encoding", "identity"),
        ];
        let response = self.0.http_request(&request).map_err(host_error)?;
        let mut bytes = Limited {
            inner: Vec::new(),
            remaining: 32 * 1024 * 1024,
        };
        self.0
            .copy_stream_to(&response.body, &mut bytes)
            .map_err(host_error)?;
        Ok(Resource {
            status: response.status,
            url: response.final_url,
            headers: response
                .headers
                .into_iter()
                .map(|header| {
                    (
                        header.name,
                        String::from_utf8_lossy(&header.value).into_owned(),
                    )
                })
                .collect(),
            bytes: bytes.inner,
        })
    }
    fn render(&self, url: &str, cancelled: &AtomicBool) -> Result<String, String> {
        let info = self
            .0
            .call_core("core.network.proxy_info", json!({}))
            .map_err(host_error)?;
        if info.get("hasCredentials").and_then(Value::as_bool) == Some(true) {
            return Err("动态渲染暂不支持需要认证的 Core 代理，请使用无需认证的本机代理".into());
        }
        let proxy: render::RenderProxy =
            decoded(info.get("settings").ok_or("Core 未返回代理配置")?)?;
        render::dump_dom_controlled(url, render::DEFAULT_WAIT_MS, cancelled, Some(&proxy))
            .map_err(|error| error.to_string())
    }
}
struct Limited<W> {
    inner: W,
    remaining: u64,
}
impl<W: Write> Write for Limited<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other("资料超过大小限制"));
        }
        let size = self.inner.write(bytes)?;
        self.remaining -= size as u64;
        Ok(size)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
fn export_file(host: &HostClient, path: &Path, name: &str) -> Result<Value, String> {
    let stream: wonderland_plugin_sdk::ByteStream = decoded(
        &host
            .call_core("core.files.export_open", json!({"name":name}))
            .map_err(host_error)?,
    )?;
    let result = (|| {
        let mut source = fs::File::open(path).map_err(|error| error.to_string())?;
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            let count = source
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            host.stream_write(&stream, &buffer[..count])
                .map_err(host_error)?;
        }
        host.call_core("core.streams.finish", json!({"streamId":stream.stream_id}))
            .map_err(host_error)
    })();
    if result.is_err() {
        let _ = host.stream_close(&stream);
    }
    result
}

fn host_error(error: PluginError) -> String {
    format!("{}: {}", error.code, error.message)
}
