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
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::DETAIL_URL;
use wonderland_plugin_sdk::PluginFailure;
use tracing::{debug, warn};

/// 默认等待渲染的虚拟时间预算（毫秒）。官方详情页是 SPA，需要等 XHR 与首屏渲染。
pub const DEFAULT_WAIT_MS: u64 = 15_000;

/// 某篇文档的详情页 URL。
pub fn detail_url(path_id: &str) -> String {
    format!("{DETAIL_URL}/{path_id}")
}

/// 用本机 Edge 无头渲染并 dump DOM。
pub fn dump_dom(url: &str, wait_ms: u64) -> Result<String, PluginFailure> {
    let edge = find_edge().ok_or_else(|| {
        PluginFailure::Other("未找到本机 Edge（msedge.exe），渲染通道不可用".to_owned())
    })?;
    let profile = unique_temp_dir();
    let mut command = Command::new(&edge);
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
    hide_child_window(&mut command);
    let output = command
        .output()
        .map_err(|error| PluginFailure::Other(format!("启动 Edge 失败：{error}")))?;
    // 无论 Edge 成功与否都清理独立 profile；并行镜像时每篇各用一份，互不争锁。
    let _ = std::fs::remove_dir_all(&profile);

    // 沙箱或首次运行会让 Edge 以非零码退出，但 DOM 已经写进 stdout（实测），故以 stdout 为准。
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(code = output.status.code(), "Edge 未产出 DOM");
        return Err(PluginFailure::Other(format!(
            "Edge 未产出 DOM（退出码 {:?}）：{}",
            output.status.code(),
            stderr.chars().take(200).collect::<String>()
        )));
    }
    if !output.status.success() {
        // 已知无害：DOM 已拿到，退出码非零只反映环境噪声。
        debug!(
            code = output.status.code(),
            bytes = stdout.len(),
            "Edge 退出码非零但 DOM 完整"
        );
    }
    Ok(stdout)
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
    for key in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Ok(base) = std::env::var(key) {
            candidates.push(PathBuf::from(base).join("Microsoft/Edge/Application/msedge.exe"));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "wonderland-knowledge-render-{}-{nanos}",
        std::process::id()
    ))
}
