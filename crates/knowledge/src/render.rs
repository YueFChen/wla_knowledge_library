//! 渲染通道：用本机 Edge 无头渲染详情页，把 DOM 交给 [`crate::page`] 抽成页面元素。
//!
//! 为什么需要它：官方目录只给标题与图标，**正文只在渲染后的页面里**；官方静态语料（`textMap.json`）
//! 又把表格压平（调研 §5），列边界与图片全丢。渲染后的 DOM 里这些结构都在，因此镜像以它为输入。
//!
//! 三点实现约定：
//! 1. 由插件启动本机 Edge 子进程渲染 `act.mihoyo.com` 的 SPA 页面，而不是用普通 HTTP
//!    抓取；渲染过程不读取用户凭据，也不外发用户数据。
//! 2. Edge 在本机可能因沙箱限制以**非零退出码**结束，但 DOM 已经完整 dump 到 stdout——
//!    因此判成功的依据是"stdout 非空"，而不是退出码（2026-09-20 实测踩到）。
//! 3. 只负责"拿到 DOM"；元素抽取在 [`crate::page`]，与这里解耦。

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::model::DETAIL_URL;
use tracing::{debug, warn};
use wonderland_plugin_sdk::PluginFailure;

/// 默认等待渲染的虚拟时间预算（毫秒）。官方详情页是 SPA，需要等 XHR 与首屏渲染。
pub const DEFAULT_WAIT_MS: u64 = 15_000;

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct RenderProxy {
    pub mode: String,
    pub address: Option<String>,
    #[serde(default)]
    pub bypass: String,
}

pub fn available() -> bool {
    find_edge().is_some()
}

/// 某篇文档的详情页 URL。
pub fn detail_url(path_id: &str) -> String {
    format!("{DETAIL_URL}/{path_id}")
}

/// 用本机 Edge 无头渲染并 dump DOM。
pub fn dump_dom(url: &str, wait_ms: u64) -> Result<String, PluginFailure> {
    dump_dom_controlled(url, wait_ms, &AtomicBool::new(false), None)
}

/// File-backed output prevents browser pipe deadlocks; cancellation kills this process tree.
pub fn dump_dom_controlled(
    url: &str,
    wait_ms: u64,
    cancelled: &AtomicBool,
    proxy: Option<&RenderProxy>,
) -> Result<String, PluginFailure> {
    let edge = find_edge().ok_or_else(|| {
        PluginFailure::Other(
            "未找到 Microsoft Edge（msedge.exe）。请确认已安装 Edge，或设置 WONDERLAND_EDGE_PATH 指向 msedge.exe；镜像渲染通道不可用"
                .to_owned(),
        )
    })?;
    let profile = unique_temp_dir();
    std::fs::create_dir_all(&profile).map_err(|error| PluginFailure::Other(error.to_string()))?;
    let output_path = profile.join("dom.html");
    let output_file = std::fs::File::create(&output_path)
        .map_err(|error| PluginFailure::Other(error.to_string()))?;
    let mut command = Command::new(&edge);
    if let Some(proxy) = proxy {
        match proxy.mode.as_str() {
            "direct" => {
                command.arg("--no-proxy-server");
            }
            "custom" => {
                let address = proxy
                    .address
                    .as_deref()
                    .ok_or_else(|| PluginFailure::Other("Core 代理地址缺失".into()))?;
                command.arg(format!("--proxy-server={address}"));
                if !proxy.bypass.is_empty() {
                    command.arg(format!(
                        "--proxy-bypass-list={}",
                        proxy.bypass.replace(',', ";")
                    ));
                }
            }
            _ => {}
        }
    }
    command.args([
        "--headless=new",
        "--disable-gpu",
        "--hide-scrollbars",
        "--mute-audio",
        // 极少数 Edge 版本会在 headless 初始化阶段短暂创建 GUI；即使如此也放到屏幕外。
        "--window-position=-32000,-32000",
        "--window-size=1,1",
        "--no-first-run",
        "--no-default-browser-check",
        &format!("--virtual-time-budget={wait_ms}"),
        &format!("--user-data-dir={}", profile.display()),
        "--dump-dom",
        url,
    ]);
    command
        .stdout(Stdio::from(output_file))
        .stderr(Stdio::null());
    hide_child_window(&mut command);
    let result = (|| {
        let mut child = command
            .spawn()
            .map_err(|error| PluginFailure::Other(format!("启动 Edge 失败：{error}")))?;
        let started = Instant::now();
        let status = loop {
            let too_large = std::fs::metadata(&output_path)
                .is_ok_and(|metadata| metadata.len() > 32 * 1024 * 1024);
            if cancelled.load(Ordering::Acquire)
                || started.elapsed() > Duration::from_secs(60)
                || too_large
            {
                kill_tree(&mut child);
                return Err(PluginFailure::Other(
                    if cancelled.load(Ordering::Acquire) {
                        "渲染已取消"
                    } else if too_large {
                        "渲染正文超过 32 MiB"
                    } else {
                        "Edge 渲染超过 60 秒"
                    }
                    .into(),
                ));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(error) => {
                    kill_tree(&mut child);
                    return Err(PluginFailure::Other(error.to_string()));
                }
            }
        };
        let stdout = std::fs::read_to_string(&output_path)
            .map_err(|error| PluginFailure::Other(error.to_string()))?;
        if stdout.trim().is_empty() {
            warn!(code = status.code(), "Edge 未产出 DOM");
            return Err(PluginFailure::Other(
                "Edge 未产出正文，请检查网页或代理配置".into(),
            ));
        }
        if !status.success() {
            debug!(
                code = status.code(),
                bytes = stdout.len(),
                "Edge 退出码非零但已产出 DOM"
            );
        }
        Ok(stdout)
    })();
    let _ = std::fs::remove_dir_all(&profile);
    result
}

fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill.exe");
        command
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_child_window(&mut command);
        let _ = command.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Windows 上禁止子进程附加控制台窗口。
///
/// `--headless` 与离屏位置负责浏览器 GUI；`CREATE_NO_WINDOW` 处理控制台子系统。
#[cfg(windows)]
fn hide_child_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_child_window(_command: &mut Command) {}

fn find_edge() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for key in ["WONDERLAND_EDGE_PATH", "EDGE_PATH"] {
        if let Some(path) = std::env::var_os(key) {
            let path = PathBuf::from(path);
            candidates.push(PathBuf::from(
                path.to_string_lossy().trim().trim_matches('"'),
            ));
        }
    }
    for key in [
        "ProgramFiles(x86)",
        "ProgramFiles",
        "ProgramW6432",
        "LOCALAPPDATA",
    ] {
        if let Ok(base) = std::env::var(key) {
            candidates.push(PathBuf::from(base).join("Microsoft/Edge/Application/msedge.exe"));
        }
    }
    if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
        return Some(path);
    }

    #[cfg(windows)]
    if let Some(path) = edge_from_app_paths() {
        return Some(path);
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join("msedge.exe"))
            .find(|path| path.is_file())
    })
}

/// Edge 可通过 App Paths 注册自定义安装目录，不能只依赖 Program Files 下的默认位置。
#[cfg(windows)]
fn edge_from_app_paths() -> Option<PathBuf> {
    const APP_PATH_KEYS: [&str; 3] = [
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe",
        r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe",
    ];
    let reg_exe = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32/reg.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("reg.exe"));

    APP_PATH_KEYS.iter().find_map(|key| {
        let mut command = Command::new(&reg_exe);
        command.args(["query", key, "/ve"]);
        hide_child_window(&mut command);
        let output = command.output().ok()?;
        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let value = stdout.lines().find_map(|line| {
            ["REG_SZ", "REG_EXPAND_SZ"].iter().find_map(|kind| {
                line.find(kind)
                    .map(|position| line[position + kind.len()..].trim().trim_matches('"'))
            })
        })?;
        let path = PathBuf::from(value);
        path.is_file().then_some(path)
    })
}

fn unique_temp_dir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "wonderland-knowledge-render-{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
