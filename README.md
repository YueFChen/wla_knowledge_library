> This is an independently versioned plugin repository. Local builds use the stable Core SDK from this checkout; see [Core repository boundaries](../../docs/REPOSITORY-BOUNDARIES.md).

# 千星奇域知识库插件

`knowledge_library` 提供知识目录更新、检索、页面镜像、本地 embedding 与 MCP 接入。它由独立 manifest、contract、React UI、Rust stdio 后端和 MCP 可执行文件组成；桌面 Core 不静态打包或装配本插件。

## 功能

- 更新官方知识目录，搜索并浏览知识条目及已镜像页面。
- 管理本地 embedding 模型，并显示索引状态。
- 按需镜像文档；`mirror.progress` 按请求 ID 关联，取消仅影响对应镜像任务。
- 生成外部 MCP 客户端的 stdio 配置片段。MCP 进程只读知识缓存，不负责抓取或修改内容。
- 只注册一个 Workspace Activity：`main`；页面直接渲染到 Core 内容区，并选择跟随 Core 主题。

## Core 能力与环境

插件申请 `network.public`、`browser.open_official`、`knowledge.local_model` 和 `files.reveal_own`。公共网络、官方链接打开、本地模型目录和缓存目录打开均由 Core 按能力代理。插件不读取账号凭据。

本地页面镜像需要系统 Edge；本地 embedding 需要下载所需模型文件。能力未授权、Edge 缺失或模型启动失败时，页面会显示可定位的错误。

## 构建和安装

在仓库根目录运行：

```powershell
pnpm build
```

生成 Windows x86_64 MSVC debug 包 `target/knowledge-library-plugin`，内含插件后端和 `backend/mcp-knowledge.exe`。在 debug Core 的“设置 → 插件管理”安装并启用该目录，然后从 Workspace 的“千星奇域知识库”主入口打开。

## 缓存与 MCP

知识目录、镜像和模型缓存统一位于 `%APPDATA%\com.wonderland.assistant\plugins\knowledge\zh-cn\v1\`。桌面 UI 与独立 MCP 进程共用此位置；升级插件不复制、清空或分叉缓存。

在插件页面的 MCP 面板中复制客户端配置，并使用包内 `backend/mcp-knowledge.exe` 的绝对路径。外部客户端直接以 stdio 启动该可执行文件；它只读共享缓存，不通过桌面 Core 调用网络服务。若将插件包移动到其他目录，需更新客户端配置中的可执行文件路径。

## 目录与契约

- `package/manifest.json`：运行时身份、兼容范围、唯一 Activity、主题集成和能力声明。
- `package/contract.json`：知识库方法、参数、超时与 `mirror.progress` 事件。
- `src/main.rs`：桌面插件协议后端及 Core 服务适配。
- `src/lib.rs`、`src/rag.rs`：桌面知识库入口和场景化检索规划；调用插件内部领域 crate。
- `crates/knowledge`：插件内部领域 crate，包含缓存、索引、镜像和检索实现，由插件后端与 MCP 入口共用。
- `mcp`：独立只读 MCP stdio 服务，与插件后端一起打入 debug 包。
- `ui/src/main.tsx`：UI Host SDK 适配；知识浏览界面及 MCP 配置面板在 `ui/src/`。

更改协议时同步更新 `package/contract.json`、`ui/src/types.generated.ts` 与后端分发。插件包由 `scripts/build-plugin.mjs` 生成；debug 包不包含发布签名。
