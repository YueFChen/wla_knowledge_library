# 知识库

`wla_knowledge_library` 是 Wonderland Assistant 的本地知识库插件：从文件、目录、网页或预设创建独立知识库，构建资料快照，检索并阅读原文。千星奇域官方资料是一个内置预设，资源浏览器独立维护。

## 工作流程

1. 创建知识库，选择空白或千星预设。
2. 添加 Markdown、TXT、HTML 文件／目录，或无需登录的网页。站点抓取限定路径、深度和页数；动态网页复用本机 Microsoft Edge。
3. 手动更新资料，保存正文、来源和配图供离线阅读；视频保留链接。完整成功扫描确认移除后才清理失效资料，失败或取消保留已成功结果。
4. 默认使用关键词检索。需要语义检索时，在全局设置添加本机嵌入服务，实际验证模型，再在当前库设置中绑定并构建语义索引。
5. 按需在 MCP 设置中勾选允许访问的库，复制外部客户端配置。

阅读工作台采用左目录、中间结果／正文、右大纲。返回检索结果保留查询、范围和滚动位置；较窄窗口收起大纲。图片受容器约束并保留原宽度，宽表格局部横向滚动。底部日志可折叠、拖动调整高度，向上查看时暂停跟随。完整任务日志只保存在最近任务的进程内存中，下一任务或进程退出即清空，不归档或导出。

## 用户自选本机模型

支持 Ollama 原生 `/api/tags`、`/api/embed`，以及 OpenAI 兼容 `/v1/models`、`/v1/embeddings` 接口。地址限本机 localhost／loopback，可配置多个服务；列表不可用时允许手填模型标识。插件不指定推荐模型，不下载、加载或升级权重，由用户的模型工具负责运行。

模型必须提供文本嵌入、固定维度、有限且非零向量。保存前真实验证返回条数和维度；可用自己的中英文和跨语言样本比较相似度。接口验证和合成协议测试不代表真实资料的召回质量。输入字符预算与查询／正文前缀在高级设置中配置，长文分段平均归一化；Ollama 显式禁用服务端截断。模型效果、输入上限与前缀按模型工具说明确认。

服务地址、认证或模型配置变更只使相关库的语义索引待重建，正文和关键词索引保留。外部工具更新同名模型后，应重新验证并重建。连接失败会明确显示原因并继续返回关键词结果。

认证由 Core 的插件密钥分区通过 Windows DPAPI 保存，服务配置不含明文。库备份只携带模型需求说明，不带地址、认证、模型文件、向量或本地来源绑定。旧的内置模型文件不会自动删除；现有资料保留，旧模型需重新绑定。

## 设置与数据

当前库设置管理本库模型与索引。全局设置按“模型服务、MCP、预设、本机”分区；技术说明按需展开。

数据使用 Core 注入的 `WONDERLAND_PLUGIN_DATA_DIR`。现有多库注册表兼容读取；首次修改旧格式前保留 `registry.before-services.json`。每个库保留最近成功快照和受管理资产。新库默认不对 MCP 暴露。删除资料源／库需要确认影响范围，仅清理库内资料，外部源文件保留。

新库支持 ZIP 导入／导出，导入后关闭 MCP 授权、解除模型绑定并重建索引，本地来源路径重新选择。声明式预设可独立分享，不执行用户脚本。本版本不接续旧版单库千星目录，也不迁移账号登录信息。

## 只读 MCP

包内包含 `backend/wla_knowledge_mcp.exe`。使用全局设置生成的配置，使 `--cache-dir` 指向实际插件数据目录。Core 退出后仍可运行，默认检索全部显式暴露的库；未暴露库不会被工具读取。成功更新和撤销授权在后续调用中生效，无需重启 MCP。

提供 `list_libraries`、`list_docs`、`read_doc`、`search`、`rag_search`，以及千星预设的 `get_node`、`search_lua_api`。所有工具只读；查询不建库、不修改索引、不管理服务。

独立 MCP 的模型请求仅连接配置的本机服务，禁用代理和重定向。认证只允许从标准 `plugin-data/wla_knowledge_library` 对应的本插件密钥分区读取，由相同 Windows 用户解密，不创建或修改密钥文件。自定义独立数据路径可用无认证服务；需要认证时通过 Core 的标准目录重新绑定。不要将认证信息放入 URL 或 MCP 参数。

## 开发与本地交付

开发版本 **0.3.0**；要求 Core **>=0.2.1, <0.3.0**，Windows x86_64 MSVC。将本仓库放到 Core checkout 的 `plugins/wla_knowledge_library`，使用 Core 的共享 SDK／UI 依赖。在本插件目录运行：

```powershell
pnpm install --frozen-lockfile
pnpm run check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo run --locked --features bindings --example generate_bindings -- --check
pnpm run build:release
```

`pnpm run build` 生成 debug 开发目录。`pnpm run build:release` 生成 `target/wla_knowledge_library-0.3.0-windows-x86_64.wplug`；可用 Core CLI `plugins install <absolute-package-path> --yes` 安装。开发包允许未签名；正式包用 `pnpm run package:release` 和原签名身份，在线分发另行确认。

CI 使用仓库变量 `WONDERLAND_CORE_COMMIT` 锁定已验证 Core 提交，沿用仓库历史。插件 ID、本地目录和 GitHub 仓库名统一为 `wla_knowledge_library`。当前只支持本机工作台／MCP，未声明远程访问。

## 代码结构

- `crates/knowledge/src/workspace/`：多库、资料构建、快照、备份、服务绑定和检索。
- `crates/knowledge/src/models.rs`：嵌入协议、验证、分段、查询缓存与独立 MCP 本机传输。
- `src/workbench.rs`：Core 服务适配、业务请求和内存日志读取。
- `mcp/`：独立 stdio 只读工具。
- `ui/src/`：三栏工作台、来源编辑、设置分区及底部虚拟日志列表。
- `package/`：manifest v3 与 NDJSON 合约；共享 PageElement 通过 Rust 生成 TypeScript。

设计依据与完整范围见 Core 文档仓库的 `WLA-KNOWLEDGE-LIBRARY-PLAN.md` 和 `WLA-KNOWLEDGE-LIBRARY-REFINEMENT-PLAN.md`，后者覆盖旧版固定模型与排版方案。
