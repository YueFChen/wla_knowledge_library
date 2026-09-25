//! 缓存：`catalog.json`（目录树）的本地留存与新鲜度判断（调研 §7.4 / §12.5）。
//!
//! 为什么只存 catalog：正文与标题锚点都由**页面镜像**提供（见 [`crate::mirror`]），
//! `headingsMap.json` / `textMap.json` 已不再需要。目录是一份小 JSON，单文件原子替换就够。
//!
//! 跨进程约定（GUI 与 MCP 二进制共享同一份缓存，ADR-010）：
//! - 目录：`<app_data>/plugins/knowledge/<lang>/v1`，与宿主 `AppContext.app_data_dir` 同源；
//! - 写入：由知识库插件经 Core 网络能力取得目录后，先写同目录临时文件再 `rename` 覆盖；
//! - 读取：整文件读完即关闭，遇到 `schema_version` 不匹配按"没有缓存"处理，避免拿旧结构跑新代码。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use wonderland_plugin_sdk::PluginFailure;
use tracing::{info, warn};

use crate::{DEFAULT_LANG, SCHEMA_VERSION};

/// 应用标识：与 `apps/desktop/src-tauri/tauri.conf.json` 的 `identifier` 一致。
///
/// 主程序的数据目录由 Tauri 按它计算（`app.path().app_data_dir()`），MCP 二进制是独立进程，
/// 只能自己拼同一路径——两处必须一致，改动 identifier 时两边都要改。
pub const APP_IDENTIFIER: &str = "com.wonderland.assistant";

/// 默认缓存新鲜度：超过它就尝试刷新（内容近似月更，调研 §4.3）。
pub const DEFAULT_MAX_AGE_DAYS: u32 = 7;

/// 一份缓存快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub schema_version: u32,
    pub lang: String,
    /// 抓取时刻的 Unix 秒。
    pub fetched_at: u64,
    /// 官方 `catalog.json` 原文。
    pub catalog: String,
}

impl Bundle {
    pub fn new(lang: &str, catalog: String) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            lang: lang.to_owned(),
            fetched_at: now_secs(),
            catalog,
        }
    }

    /// 已存留的天数。
    pub fn age_days(&self) -> f64 {
        now_secs().saturating_sub(self.fetched_at) as f64 / 86_400.0
    }

    pub fn is_fresh(&self, max_age_days: u32) -> bool {
        self.age_days() <= f64::from(max_age_days)
    }
}

/// 缓存目录。
pub struct KnowledgeCache {
    dir: PathBuf,
}

impl KnowledgeCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// 默认位置：`<app_data>/plugins/knowledge/<lang>/v1`。
    ///
    /// 可用 `WONDERLAND_APP_DATA_DIR` 覆盖应用数据目录（开发与测试用）。
    pub fn for_lang(lang: &str) -> Self {
        Self::in_app_data(&app_data_dir(), lang)
    }

    /// 指定应用数据目录下的默认位置：`<app_data_dir>/plugins/knowledge/<lang>/v1`。
    ///
    /// GUI 侧（插件）手里的是宿主的 `AppContext.app_data_dir`，用它构造才能与
    /// MCP 二进制用 [`Self::for_lang`] 算出的目录**是同一份**（ADR-010 的跨进程约定）。
    pub fn in_app_data(app_data_dir: &Path, lang: &str) -> Self {
        Self::new(
            app_data_dir
                .join("plugins")
                .join("knowledge")
                .join(lang)
                .join("v1"),
        )
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn bundle_path(&self) -> PathBuf {
        self.dir.join("bundle.json")
    }

    /// 读缓存；文件不存在或结构版本不匹配时返回 `None`。
    pub fn load(&self) -> Result<Option<Bundle>, PluginFailure> {
        let path = self.bundle_path();
        if !path.is_file() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .map_err(|error| PluginFailure::LocalData(format!("读缓存失败：{error}")))?;
        let bundle: Bundle = serde_json::from_str(&raw)
            .map_err(|error| PluginFailure::LocalData(format!("缓存解析失败：{error}")))?;
        if bundle.schema_version != SCHEMA_VERSION {
            warn!(
                found = bundle.schema_version,
                expected = SCHEMA_VERSION,
                "缓存结构版本不匹配，按无缓存处理"
            );
            return Ok(None);
        }
        Ok(Some(bundle))
    }

    /// 原子写入：同目录临时文件 → rename 覆盖。
    ///
    /// Windows 上若另一进程正打开目标文件，rename 会失败（共享冲突），因此重试几次；
    /// 读侧"整文件读完即关闭"让这个窗口非常短。
    pub fn store(&self, bundle: &Bundle) -> Result<(), PluginFailure> {
        fs::create_dir_all(&self.dir)
            .map_err(|error| PluginFailure::LocalData(format!("创建缓存目录失败：{error}")))?;
        let bytes = serde_json::to_vec(bundle)
            .map_err(|error| PluginFailure::LocalData(format!("缓存序列化失败：{error}")))?;
        let temp = self.dir.join(format!(
            "bundle.json.tmp-{}-{}",
            std::process::id(),
            now_secs()
        ));
        fs::write(&temp, &bytes)
            .map_err(|error| PluginFailure::LocalData(format!("写缓存临时文件失败：{error}")))?;

        let target = self.bundle_path();
        let mut last = None;
        for attempt in 0..5 {
            match fs::rename(&temp, &target) {
                Ok(()) => {
                    info!(bytes = bytes.len(), dir = %self.dir.display(), "知识库缓存已更新");
                    return Ok(());
                }
                Err(error) => {
                    last = Some(error);
                    std::thread::sleep(Duration::from_millis(40 * (attempt + 1)));
                }
            }
        }
        let _ = fs::remove_file(&temp);
        Err(PluginFailure::LocalData(format!(
            "替换缓存失败（可能被其它进程占用）：{}",
            last.map(|error| error.to_string()).unwrap_or_default()
        )))
    }

}

/// 应用数据目录（与宿主 `AppContext.app_data_dir` 同源）。
pub fn app_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("WONDERLAND_APP_DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join(APP_IDENTIFIER);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(APP_IDENTIFIER);
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(data) = std::env::var("XDG_DATA_HOME") {
            return PathBuf::from(data).join(APP_IDENTIFIER);
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(APP_IDENTIFIER);
        }
    }
    std::env::temp_dir().join(APP_IDENTIFIER)
}

/// 默认语种（`lang` 参数缺省时使用）。
pub fn default_lang() -> &'static str {
    DEFAULT_LANG
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_in_temp(name: &str) -> KnowledgeCache {
        let dir = std::env::temp_dir().join(format!(
            "wonderland-knowledge-cache-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        KnowledgeCache::new(dir)
    }

    #[test]
    fn store_and_load_roundtrip() {
        let cache = cache_in_temp("roundtrip");
        assert!(cache.load().unwrap().is_none(), "首次应为空缓存");

        let bundle = Bundle::new("zh-cn", "[]".into());
        cache.store(&bundle).unwrap();

        let loaded = cache.load().unwrap().expect("缓存应可读回");
        assert_eq!(loaded.schema_version, SCHEMA_VERSION);
        assert_eq!(loaded.catalog, "[]");
        assert!(loaded.is_fresh(DEFAULT_MAX_AGE_DAYS));

        // 不留临时文件（原子替换的副作用要清干净）
        let leftovers: Vec<_> = fs::read_dir(cache.dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件");
    }

    #[test]
    fn incompatible_schema_is_treated_as_missing() {
        let cache = cache_in_temp("schema");
        cache.store(&Bundle::new("zh-cn", "[]".into())).unwrap();
        // 手工改成未来版本
        let path = cache.dir().join("bundle.json");
        let raw = fs::read_to_string(&path)
            .unwrap()
            .replace("\"schema_version\":2", "\"schema_version\":99");
        fs::write(&path, raw).unwrap();
        assert!(cache.load().unwrap().is_none(), "版本不匹配应按无缓存处理");
    }

}
