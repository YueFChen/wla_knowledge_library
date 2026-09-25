import type {
  KnowledgeDocGroup,
  KnowledgeEmbeddingStatus,
  KnowledgeMirrorStatus,
  KnowledgeRefreshReport,
  KnowledgeSearchGroup,
  KnowledgeStatus,
  MirrorProgress,
  MirrorRun,
  MirroredPage,
} from './types.generated'

/** 插件前端与宿主之间的契约；实现由 shell 提供（见 apps/desktop）。 */
export interface KnowledgeApi {
  /** 缓存与索引状态；没有缓存不是错误，靠 `ready = false` + `error` 表达。 */
  status(): Promise<KnowledgeStatus>
  embeddingStatus(): Promise<KnowledgeEmbeddingStatus>
  activateEmbeddings(): Promise<KnowledgeEmbeddingStatus>
  disableEmbeddings(): Promise<KnowledgeEmbeddingStatus>
  recheckEmbeddings(): Promise<KnowledgeEmbeddingStatus>
  /** 获取 catalog 并落缓存（界面上的「获取目录」/「更新目录」）。 */
  refresh(): Promise<KnowledgeRefreshReport>
  /** 插件按场景自动规划 RAG，调用方只需提交自然语言问题。 */
  search(queries: string[], topK: number): Promise<KnowledgeSearchGroup[]>
  /** 列文档标题；关键词留空即全部（目录树用）。 */
  docs(keywords: string[]): Promise<KnowledgeDocGroup[]>
  /** 用系统默认浏览器打开某篇的官方原文；只传 `path_id`，不传地址。 */
  openSource(pathId: string): Promise<void>
  /** 用系统默认浏览器打开内置模型的受信任来源页。 */
  openModelSource(source: 'onnx' | 'model'): Promise<void>
  /** 在系统文件管理器里打开缓存目录。 */
  revealCacheDir(): Promise<void>
  /** 页面镜像状态。 */
  mirrorStatus(): Promise<KnowledgeMirrorStatus>
  /** 批量镜像；逐篇回调进度，返回本次结果。 */
  mirrorStart(limit: number | null, onProgress: (progress: MirrorProgress) => void): Promise<MirrorRun>
  /** 请正在跑的镜像停下。 */
  mirrorCancel(): Promise<void>
  /** 读一篇已镜像的页面；没镜像过为 `null`。 */
  page(pathId: string): Promise<MirroredPage | null>
  /** 镜像单篇并返回结果。 */
  mirrorPage(pathId: string): Promise<MirroredPage>
}
