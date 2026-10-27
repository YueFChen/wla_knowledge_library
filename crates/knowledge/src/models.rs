//! User-selected local embedding services. Model assets are managed outside the plugin.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub type Result<T> = std::result::Result<T, String>;
pub const PLUGIN_ID: &str = "wla_knowledge_library";
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn input_chars() -> usize {
    1000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub secret_id: Option<String>,
    #[serde(default)]
    pub revision: String,
}
impl Service {
    pub fn validate(&self) -> Result<()> {
        let url = url::Url::parse(&self.url).map_err(|_| "模型服务地址无效")?;
        let local = match url.host() {
            Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        };
        if !local
            || !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("请选择本机 HTTP(S) 模型服务，不在地址中填写认证信息".into());
        }
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || !matches!(self.kind.as_str(), "ollama" | "openai")
            || (!self.id.is_empty() && !crate::workspace::valid_id(&self.id))
        {
            return Err("模型服务配置无效".into());
        }
        if self
            .secret_id
            .as_ref()
            .is_some_and(|key| key != &format!("embedding_{}", self.id))
        {
            return Err("模型认证身份无效".into());
        }
        Ok(())
    }
    pub fn endpoint(&self, suffix: &str) -> Result<String> {
        self.validate()?;
        let base = self.url.trim_end_matches('/');
        let prefix = if self.kind == "ollama" {
            if base.ends_with("/api") {
                base.to_string()
            } else {
                format!("{base}/api")
            }
        } else if url::Url::parse(base).map_err(|_| "模型地址无效")?.path() == "/" {
            format!("{base}/v1")
        } else {
            base.into()
        };
        Ok(format!("{prefix}/{suffix}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfile {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub service_id: String,
    pub model: String,
    #[serde(default)]
    pub dimensions: usize,
    #[serde(default = "input_chars")]
    pub max_chars: usize,
    #[serde(default)]
    pub query_prefix: String,
    #[serde(default)]
    pub document_prefix: String,
    #[serde(default)]
    pub revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequirement {
    pub name: String,
    pub model: String,
    pub dimensions: usize,
    pub max_chars: usize,
    pub query_prefix: String,
    pub document_prefix: String,
}
impl From<&ModelProfile> for ModelRequirement {
    fn from(value: &ModelProfile) -> Self {
        Self {
            name: value.name.clone(),
            model: value.model.clone(),
            dimensions: value.dimensions,
            max_chars: value.max_chars,
            query_prefix: value.query_prefix.clone(),
            document_prefix: value.document_prefix.clone(),
        }
    }
}
impl ModelProfile {
    pub fn validate(&self, tested: bool) -> Result<()> {
        if !crate::workspace::valid_id(&self.service_id)
            || self.name.trim().is_empty()
            || self.name.len() > 128
            || self.model.trim().is_empty()
            || self.model.len() > 256
            || self.model.chars().any(char::is_control)
            || !(64..=16000).contains(&self.max_chars)
            || self.query_prefix.len() > 4096
            || self.document_prefix.len() > 4096
            || self.query_prefix.chars().count() >= self.max_chars
            || self.document_prefix.chars().count() >= self.max_chars
            || self.dimensions > 32768
            || (tested && self.dimensions == 0)
        {
            return Err("模型参数无效，请检查标识、输入长度和向量维度".into());
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct ResolvedModel {
    pub profile: ModelProfile,
    pub service: Service,
}
impl ResolvedModel {
    pub fn fingerprint(&self) -> String {
        digest(
            &serde_json::to_vec(&(
                "embedding-service-v1/mean-normalized",
                &self.profile,
                &self.service,
            ))
            .expect("model configuration"),
        )
    }
}

pub trait Transport: Send + Sync {
    fn request(
        &self,
        service: &Service,
        method: &str,
        url: &str,
        body: Option<&Value>,
        cancelled: &AtomicBool,
    ) -> Result<Value>;
}

/// Only the standard Core plugin-data layout grants a path to this plugin's encrypted secrets.
pub fn secret_root(cache: &Path) -> Option<PathBuf> {
    (cache.file_name()?.to_str()? == PLUGIN_ID
        && cache.parent()?.file_name()?.to_str()? == "plugin-data")
        .then(|| {
            cache
                .parent()?
                .parent()
                .map(|root| root.join("plugin-secrets").join(PLUGIN_ID))
        })?
}
pub fn read_credential(cache: &Path, service: &Service) -> Result<Option<String>> {
    service.validate()?;
    let Some(key) = &service.secret_id else {
        return Ok(None);
    };
    let directory =
        secret_root(cache).ok_or("此数据目录未关联本机认证，请在 Core 中重新绑定服务")?;
    let path = directory.join(format!("{key}.dpapi"));
    let metadata =
        std::fs::metadata(&path).map_err(|_| "模型服务认证缺失，请在 Core 中重新配置")?;
    if !metadata.is_file() || metadata.len() > 65536 {
        return Err("模型服务认证文件无效".into());
    }
    let sealed = std::fs::read(path).map_err(|_| "无法读取本机模型认证")?;
    let plain = wonderland_secret::unseal(&sealed)
        .map_err(|_| "此用户无法读取模型认证，请在 Core 中重新配置")?;
    String::from_utf8(plain)
        .map(Some)
        .map_err(|_| "模型认证格式无效".into())
}

pub struct NativeTransport<'a>(pub &'a Path);
impl Transport for NativeTransport<'_> {
    fn request(
        &self,
        service: &Service,
        method: &str,
        url: &str,
        body: Option<&Value>,
        cancelled: &AtomicBool,
    ) -> Result<Value> {
        service.validate()?;
        if cancelled.load(Ordering::Acquire) {
            return Err("任务已取消".into());
        }
        let credential = read_credential(self.0, service)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "无法启动本机模型连接")?;
        runtime.block_on(async {
            let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never()).timeout(std::time::Duration::from_secs(90)).build().map_err(|_|"无法创建本机模型连接")?;
            let mut request=client.request(reqwest::Method::from_bytes(method.as_bytes()).map_err(|_|"无效请求方法")?,url);
            if let Some(key)=credential { request=request.bearer_auth(key); }
            if let Some(body)=body { request=request.json(body); }
            let operation=async {
                let mut response=request.send().await.map_err(|_|"本机模型连接失败，请检查服务和地址")?;
                if !response.status().is_success() { return Err(format!("模型服务 HTTP {}",response.status().as_u16())); }
                let mut bytes=Vec::new();
                while let Some(chunk)=response.chunk().await.map_err(|_|"模型响应读取中断")? {
                    if bytes.len()+chunk.len()>32*1024*1024 { return Err("模型响应超过 32 MiB".into()); }
                    bytes.extend_from_slice(&chunk);
                }
                serde_json::from_slice(&bytes).map_err(|_|"模型服务未返回有效 JSON".into())
            };
            tokio::pin!(operation);
            let mut tick=tokio::time::interval(std::time::Duration::from_millis(100));
            loop { tokio::select! { result=&mut operation => return result, _=tick.tick()=>{ if cancelled.load(Ordering::Acquire) { return Err("任务已取消".into()); } } } }
        })
    }
}

pub fn list_models(
    service: &Service,
    transport: &dyn Transport,
    cancelled: &AtomicBool,
) -> Result<Vec<String>> {
    let path = if service.kind == "ollama" {
        "tags"
    } else {
        "models"
    };
    let value = transport.request(service, "GET", &service.endpoint(path)?, None, cancelled)?;
    let rows = value[if service.kind == "ollama" {
        "models"
    } else {
        "data"
    }]
    .as_array()
    .ok_or("服务不提供模型列表，请手填模型标识")?;
    let mut names = rows
        .iter()
        .filter_map(|row| {
            row[if service.kind == "ollama" {
                "name"
            } else {
                "id"
            }]
            .as_str()
            .map(String::from)
        })
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    Ok(names)
}

fn normalize(mut vector: Vec<f32>, dimensions: usize) -> Result<Vec<f32>> {
    if vector.is_empty()
        || vector.len() > 32768
        || (dimensions > 0 && vector.len() != dimensions)
        || vector.iter().any(|v| !v.is_finite())
    {
        return Err("模型返回的向量维度或数值无效，请重新验证模型并重建索引".into());
    }
    let norm = vector
        .iter()
        .map(|v| (*v as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm <= f64::EPSILON || !norm.is_finite() {
        return Err("模型返回零向量或无效向量".into());
    }
    for v in &mut vector {
        *v = (*v as f64 / norm) as f32;
    }
    Ok(vector)
}

fn request_vectors(
    model: &ResolvedModel,
    inputs: &[String],
    transport: &dyn Transport,
    cancelled: &AtomicBool,
) -> Result<Vec<Vec<f32>>> {
    if cancelled.load(Ordering::Acquire) {
        return Err("任务已取消".into());
    }
    let input = if inputs.len() == 1 {
        json!(inputs[0])
    } else {
        json!(inputs)
    };
    let mut body = json!({"model":model.profile.model,"input":input});
    let path = if model.service.kind == "ollama" {
        body["truncate"] = json!(false);
        "embed"
    } else {
        "embeddings"
    };
    let value = transport.request(
        &model.service,
        "POST",
        &model.service.endpoint(path)?,
        Some(&body),
        cancelled,
    )?;
    let vectors = if model.service.kind == "ollama" {
        serde_json::from_value::<Vec<Vec<f32>>>(value["embeddings"].clone())
            .map_err(|_| "此模型未提供有效文本嵌入")?
    } else {
        let mut rows = value["data"]
            .as_array()
            .ok_or("此模型未提供有效文本嵌入")?
            .iter()
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row["index"].as_u64().unwrap_or(u64::MAX));
        if rows
            .iter()
            .enumerate()
            .any(|(index, row)| row["index"].as_u64() != Some(index as u64))
        {
            return Err("嵌入响应序号无效".into());
        }
        rows.into_iter()
            .map(|row| {
                serde_json::from_value::<Vec<f32>>(row["embedding"].clone())
                    .map_err(|_| "模型嵌入格式无效".into())
            })
            .collect::<Result<Vec<_>>>()?
    };
    if vectors.len() != inputs.len() {
        return Err("模型返回的嵌入数量与文本数量不一致".into());
    }
    vectors
        .into_iter()
        .map(|v| normalize(v, model.profile.dimensions))
        .collect()
}

#[derive(Default)]
pub struct ModelPool {
    queries: Mutex<HashMap<String, Vec<f32>>>,
}
impl ModelPool {
    pub fn embed(
        &self,
        model: &ResolvedModel,
        texts: &[String],
        query: bool,
        transport: &dyn Transport,
        cancelled: &AtomicBool,
    ) -> Result<Vec<Vec<f32>>> {
        model.profile.validate(false)?;
        model.service.validate()?;
        let prefix = if query {
            &model.profile.query_prefix
        } else {
            &model.profile.document_prefix
        };
        let budget = model.profile.max_chars - prefix.chars().count();
        let mut parts = Vec::new();
        let mut counts = Vec::new();
        for text in texts {
            if text.trim().is_empty() {
                return Err("无法向量化空文本".into());
            }
            let chars = text.chars().collect::<Vec<_>>();
            let start = parts.len();
            parts.extend(
                chars
                    .chunks(budget)
                    .map(|part| format!("{prefix}{}", part.iter().collect::<String>())),
            );
            counts.push(parts.len() - start);
        }
        let mut encoded = Vec::new();
        for batch in parts.chunks(8) {
            match request_vectors(model, batch, transport, cancelled) {
                Ok(vectors) => encoded.extend(vectors),
                Err(error)
                    if batch.len() > 1
                        && (error.contains("HTTP 400") || error.contains("HTTP 422")) =>
                {
                    for part in batch {
                        encoded.extend(request_vectors(
                            model,
                            std::slice::from_ref(part),
                            transport,
                            cancelled,
                        )?);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        let dimensions = encoded
            .first()
            .map(Vec::len)
            .unwrap_or(model.profile.dimensions);
        if encoded.iter().any(|vector| vector.len() != dimensions) {
            return Err("同一模型的向量维度发生变化".into());
        }
        let mut offset = 0;
        let mut result = Vec::new();
        for count in counts {
            let mut aggregate = vec![0.0; dimensions];
            for vector in &encoded[offset..offset + count] {
                for (sum, v) in aggregate.iter_mut().zip(vector) {
                    *sum += v / count as f32;
                }
            }
            result.push(normalize(aggregate, dimensions)?);
            offset += count;
        }
        Ok(result)
    }
    pub fn query(
        &self,
        model: &ResolvedModel,
        text: &str,
        transport: &dyn Transport,
        cancelled: &AtomicBool,
    ) -> Result<Vec<f32>> {
        let key = digest(format!("{}:{text}", model.fingerprint()).as_bytes());
        if let Some(vector) = self.queries.lock().map_err(|_| "查询状态不可用")?.get(&key) {
            return Ok(vector.clone());
        }
        let vector = self
            .embed(model, &[text.into()], true, transport, cancelled)?
            .remove(0);
        let mut cache = self.queries.lock().map_err(|_| "查询状态不可用")?;
        if cache.len() >= 128 {
            cache.clear();
        }
        cache.insert(key, vector.clone());
        Ok(vector)
    }
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.queries.lock() {
            cache.clear();
        }
    }
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
