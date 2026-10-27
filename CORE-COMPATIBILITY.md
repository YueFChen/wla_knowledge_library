# Core 兼容说明

知识库开发版本 **0.3.0** 要求 Core **>=0.2.1, <0.3.0**，仅支持 Windows x86_64 MSVC。本地目录、运行 ID 和 GitHub 仓库统一为 `wla_knowledge_library`。

保持 manifest v3、NDJSON 1.0、UI Bridge 1.x。使用每次调用传入的 `HostClient` 保留 Core 服务上下文与取消关系。用到统一 HTTP／字节流、插件密钥、浏览器打开、文件导出、通用文件／目录选择和代理信息接口。动态网页复用本机 Edge，语义推理由用户自备本机 Ollama／OpenAI 兼容嵌入服务承担。

Core 0.2.1 已发布，包含 `core.files.pick_paths` 和 `core.network.proxy_info`。普通 CI 的 `WONDERLAND_CORE_COMMIT` 固定为验证过的 Core 完整提交；本轮基线为 `0cdace0573e555d394ce9dcdf72edf44da0ff9ee`。

`backend.supportsServiceContext: true`；不缓存宿主客户端跨请求执行文件操作。插件未声明远程访问，Core 不对远程会话提供此插件。

独立 MCP 与 GUI 共用领域代码，读取显式暴露的库。GUI 模型请求经 Core HTTP 与密钥头注入；独立 MCP 使用 loopback HTTP，在相同 Windows 用户下只读解密本插件标准密钥分区，不要求 Core 持续运行。

本地开发包允许未签名。本轮交付本地验收包和源码；正式签名与 Cloudflare 发布另行确认，不将版本号视为已在线发行。
