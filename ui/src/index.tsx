import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type {
  KnowledgeMirrorStatus,
  KnowledgeRefreshReport,
  KnowledgeSearchGroup,
  KnowledgeStatus,
  MirroredPage,
} from './types.generated'

import type { KnowledgeApi } from './api'
import { anchorId, errorMessage, pageHeadings, scrollToText } from './display'
import { DocReader } from './DocReader'
import { DocTree } from './DocTree'
import { EmbeddingGuide } from './EmbeddingGuide'
import { t } from './i18n'
import { McpPanel } from './McpPanel'
import { Outline } from './Outline'
import { SearchResults } from './SearchResults'
import { Toolbar } from './Toolbar'
import './style.css'

export type { KnowledgeApi } from './api'

interface Props {
  api: KnowledgeApi
}

/** 每个问题最多返回多少条：与 MCP 工具的默认值一致（工具面声明 1–20）。 */
const TOP_K = 5

/** 打开的文档：镜像结果 + 打开来源（未镜像时也能显示出标题）。 */
interface OpenDoc {
  pathId: string
  title: string | null
  /** 从检索结果点进来时的命中锚点，镜像读回来后跳过去。 */
  anchor?: string
  /** 命中块正文：镜像加载后用于精确定位到段落、列表项或表格行。 */
  targetText?: string
  page: MirroredPage | null
  loading: boolean
}

interface NavigationSnapshot {
  view: OpenDoc | null
  results: KnowledgeSearchGroup[] | null
  query: string
  scrollTop: number
}

/**
 * 知识库管理页，照官方教程详情页的样子排：顶部钉住工具区与检索栏，
 * 下面三栏——左目录树 / 中正文 / 右文章大纲，目录与大纲跟着窗口锁定，只有正文上下滚。
 *
 * 目录与大纲一起钉住的原因：它们分别是「去哪一篇」和「在这篇的哪一节」两个导航面，
 * 让它们跟着正文一起滚，读到中段就用不了了；正文独占滚动条，滚动位置只属于内容本身。
 *
 * 会话级状态（状态 / 镜像统计 / 打开的文档 / 检索结果 / 目录树重取标记）收在这里，
 * 工具区与目录树只管各自的输入与中间态。
 */
export function KnowledgeLibraryPage({ api }: Props) {
  const [status, setStatus] = useState<KnowledgeStatus | null>(null)
  const [mirror, setMirror] = useState<KnowledgeMirrorStatus | null>(null)
  const [report, setReport] = useState<KnowledgeRefreshReport | null>(null)
  const [view, setView] = useState<OpenDoc | null>(null)
  const [results, setResults] = useState<KnowledgeSearchGroup[] | null>(null)
  const [navigationHistory, setNavigationHistory] = useState<NavigationSnapshot[]>([])
  const [query, setQuery] = useState('')
  const [searching, setSearching] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [mirroringDoc, setMirroringDoc] = useState(false)
  const [mcpOpen, setMcpOpen] = useState(false)
  const [embeddingOpen, setEmbeddingOpen] = useState(false)
  const [treeToken, setTreeToken] = useState(0)
  const [error, setError] = useState('')
  const mounted = useRef(true)
  const pendingScrollTop = useRef<number | null>(null)
  /** 正文的滚动容器：大纲按它的滚动位置高亮，换篇目时也要把它拉回顶部。 */
  const mainRef = useRef<HTMLDivElement | null>(null)

  const headings = useMemo(() => (view?.page ? pageHeadings(view.page) : []), [view?.page])
  // 只有一张标题的篇目没有可导航的层级，右侧栏留给正文更划算。
  const outline = headings.length > 1

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])

  const loadMirror = useCallback(async () => {
    try {
      const next = await api.mirrorStatus()
      if (mounted.current) setMirror(next)
    } catch {
      // 镜像统计读不出来不该影响检索与读文档：工具区少一行数字而已。
    }
  }, [api])

  const loadStatus = useCallback(async () => {
    const next = await api.status()
    if (mounted.current) setStatus(next)
    return next
  }, [api])

  useEffect(() => {
    let live = true
    void loadStatus()
      .then((next) => {
        if (live) setStatus(next)
      })
      .catch((cause) => {
        if (live) setError(errorMessage(cause, t('common.failed')))
      })
    return () => {
      live = false
    }
  }, [loadStatus])

  useEffect(() => {
    // catalog 已落盘就能列出镜像目标；不能等 `ready`，因为 ready 本身要求至少已有一篇镜像。
    if (status?.cached_at !== null && status?.cached_at !== undefined) void loadMirror()
  }, [status?.cached_at, loadMirror])

  useEffect(() => {
    // 页面重载或切回插件后，原来的事件订阅已经不存在；运行中的后台任务改由状态轮询接续展示。
    if (!mirror?.running) return
    let live = true
    const timer = window.setInterval(() => {
      void api
        .mirrorStatus()
        .then(async (next) => {
          if (!live) return
          setMirror(next)
          if (!next.running) {
            await loadStatus()
            if (live) setTreeToken((token) => token + 1)
          }
        })
        .catch(() => undefined)
    }, 750)
    return () => {
      live = false
      window.clearInterval(timer)
    }
  }, [api, loadStatus, mirror?.running])

  // 换文档时先回到顶部，之后再定位命中内容或标题锚点。
  useEffect(() => {
    if (!mainRef.current) return
    mainRef.current.scrollTop = 0
    if (pendingScrollTop.current !== null) {
      const scrollTop = pendingScrollTop.current
      pendingScrollTop.current = null
      window.requestAnimationFrame(() => {
        if (mainRef.current) mainRef.current.scrollTop = scrollTop
      })
    }
  }, [view?.pathId, results])

  // 检索命中先按正文块文本定位到段落/列表项/表格行，精确匹配不到时再退回标题锚点。
  useEffect(() => {
    if (!view?.anchor || !view.page) return
    if (view.targetText && mainRef.current && scrollToText(mainRef.current, view.targetText)) return
    document.getElementById(anchorId(view.anchor))?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }, [view])

  const snapshot = useCallback(
    (): NavigationSnapshot => ({ view, results, query, scrollTop: mainRef.current?.scrollTop ?? 0 }),
    [view, results, query],
  )

  const goBack = useCallback(() => {
    const previous = navigationHistory[navigationHistory.length - 1]
    if (!previous) {
      setView(null)
      return
    }
    pendingScrollTop.current = previous.scrollTop
    setNavigationHistory((history) => history.slice(0, -1))
    setView(previous.view)
    setResults(previous.results)
    setQuery(previous.query)
  }, [navigationHistory])

  const refresh = useCallback(async () => {
    setRefreshing(true)
    setError('')
    try {
      const result = await api.refresh()
      if (!mounted.current) return
      setReport(result)
      setStatus(result.status)
      // 缓存换了：手上的正文与检索结果都是旧语料里的，目录也可能变了，一并退回。
      setView(null)
      setResults(null)
      setNavigationHistory([])
      setTreeToken((token) => token + 1)
      await loadMirror()
    } catch (cause) {
      if (mounted.current) setError(errorMessage(cause, t('common.failed')))
    } finally {
      if (mounted.current) setRefreshing(false)
    }
  }, [api, loadMirror])

  const search = useCallback(async () => {
    const normalizedQuery = query.trim()
    if (normalizedQuery.length === 0) {
      setError(t('search.needQuery'))
      return
    }
    setSearching(true)
    setError('')
    try {
      const groups = await api.search([normalizedQuery], TOP_K)
      if (!mounted.current) return
      if (view || results) setNavigationHistory((history) => [...history, snapshot()])
      setResults(groups)
      // 输入驱动的全库匹配：不要求用户先选专题或范围。
      setView(null)
    } catch (cause) {
      if (mounted.current) setError(errorMessage(cause, t('common.failed')))
    } finally {
      if (mounted.current) setSearching(false)
    }
  }, [api, query, results, snapshot, view])

  const reveal = useCallback(async () => {
    setError('')
    try {
      await api.revealCacheDir()
    } catch (cause) {
      setError(errorMessage(cause, t('common.failed')))
    }
  }, [api])

  const openSource = useCallback(
    async (pathId: string) => {
      setError('')
      try {
        await api.openSource(pathId)
      } catch (cause) {
        setError(errorMessage(cause, t('common.failed')))
      }
    },
    [api],
  )

  /** 打开一篇：先摆上标题（镜像读得慢），镜像读回来再就地换成官方排版。 */
  const openDoc = useCallback(
    async (pathId: string, title: string | null, anchor?: string, targetText?: string) => {
      setError('')
      setNavigationHistory((history) => [...history, snapshot()])
      setView({ pathId, title, anchor, targetText, page: null, loading: true })
      try {
        const page = await api.page(pathId)
        if (mounted.current) setView({ pathId, title, anchor, targetText, page, loading: false })
      } catch (cause) {
        if (mounted.current) {
          setView(null)
          setError(errorMessage(cause, t('common.failed')))
        }
      }
    },
    [api, snapshot],
  )

  const mirrorDoc = useCallback(
    async (pathId: string) => {
      setMirroringDoc(true)
      setError('')
      try {
        const page = await api.mirrorPage(pathId)
        if (!mounted.current) return
        // 镜像完就地换成官方排版，不重开一篇（重开会丢掉刚滚到的位置）。
        setView((current) => (current && current.pathId === pathId ? { ...current, page, loading: false } : current))
        await loadMirror()
      } catch (cause) {
        if (mounted.current) setError(errorMessage(cause, t('common.failed')))
      } finally {
        if (mounted.current) setMirroringDoc(false)
      }
    },
    [api, loadMirror],
  )

  const onMirrorChanged = useCallback(() => {
    // 首次镜像后 `ready` 会从 false 变为 true；只刷新镜像统计会让正文区一直被锁住。
    void Promise.all([loadMirror(), loadStatus()])
    setTreeToken((token) => token + 1)
  }, [loadMirror, loadStatus])

  return (
    <section className="ka">
      {!status ? (
        <p className="ka-placeholder" role="status">
          {t('common.loading')}
        </p>
      ) : (
        <>
          <Toolbar
            api={api}
            status={status}
            report={report}
            mirror={mirror}
            refreshing={refreshing}
            mcpOpen={mcpOpen}
            embeddingOpen={embeddingOpen}
            onRefresh={() => void refresh()}
            onReveal={() => void reveal()}
            onToggleMcp={() => {
              setMcpOpen((open) => !open)
              setEmbeddingOpen(false)
            }}
            onToggleEmbedding={() => {
              setEmbeddingOpen((open) => !open)
              setMcpOpen(false)
            }}
            onChanged={onMirrorChanged}
          />

          {error && (
            <div className="ka-error" role="alert">
              {error}
            </div>
          )}

          {mcpOpen && <McpPanel status={status} onClose={() => setMcpOpen(false)} />}
          {embeddingOpen && (
            <EmbeddingGuide
              api={api}
              cacheDir={status.cache_dir}
              onOpenCache={() => void reveal()}
              onClose={() => setEmbeddingOpen(false)}
            />
          )}

          {status.cached_at !== null && (
            <>
              {status.ready && (
                <>
                  <div className="ka-search">
                    <input
                      className="ka-input"
                      aria-label={t('search.placeholder')}
                      type="search"
                      value={query}
                      placeholder={t('search.placeholder')}
                      onChange={(event) => setQuery(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter') {
                          event.preventDefault()
                          void search()
                        }
                      }}
                    />
                    <button className="ka-primary" disabled={searching} onClick={() => void search()}>
                      {searching ? t('search.searching') : t('search.submit')}
                    </button>
                  </div>
                </>
              )}

              <div className={outline ? 'ka-body ka-body--outline' : 'ka-body'}>
                <DocTree
                  api={api}
                  reloadToken={treeToken}
                  activePathId={view?.pathId ?? null}
                  onOpen={(pathId, title) => void openDoc(pathId, title)}
                />

                <div className="ka-main" ref={mainRef}>
                  {view ? (
                    <DocReader
                      pathId={view.pathId}
                      title={view.title}
                      page={view.page}
                      loading={view.loading}
                      mirroring={mirroringDoc}
                      onMirror={(pathId) => void mirrorDoc(pathId)}
                      onBack={goBack}
                      onOpenSource={(pathId) => void openSource(pathId)}
                      onOpenDoc={(pathId) => void openDoc(pathId, null)}
                    />
                  ) : results ? (
                    <SearchResults
                      groups={results}
                      canGoBack={navigationHistory.length > 0}
                      onBack={goBack}
                      onOpen={(pathId, title, anchor, text) => void openDoc(pathId, title, anchor, text)}
                    />
                  ) : (
                    <p className="ka-placeholder">{t('doc.placeholder')}</p>
                  )}
                </div>

                {outline && <Outline headings={headings} scrollRef={mainRef} />}
              </div>
            </>
          )}
        </>
      )}
    </section>
  )
}
