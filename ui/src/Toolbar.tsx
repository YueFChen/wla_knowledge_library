import { useState } from 'react'
import type {
  KnowledgeMirrorStatus,
  KnowledgeRefreshReport,
  KnowledgeStatus,
  MirrorProgress,
  MirrorRun,
} from './types.generated'
import { BrainCircuit, FolderOpen, HardDriveDownload, Plug, RefreshCw, Square } from 'lucide-react'

import type { KnowledgeApi } from './api'
import { errorMessage, formatBytes, formatTime, wholeDays } from './display'
import { t } from './i18n'

interface Props {
  api: KnowledgeApi
  status: KnowledgeStatus
  /** 最近一次抓取的结果；没抓过为 null。 */
  report: KnowledgeRefreshReport | null
  /** 镜像统计；读不出来时为 null（工具区照常可用）。 */
  mirror: KnowledgeMirrorStatus | null
  refreshing: boolean
  mcpOpen: boolean
  embeddingOpen: boolean
  onRefresh: () => void
  onReveal: () => void
  onToggleMcp: () => void
  onToggleEmbedding: () => void
  /** 一次镜像跑完后刷新镜像统计（目录树也该跟着重取）。 */
  onChanged: () => void
}

/**
 * 顶部工具区：一行状态摘要 + 一行操作按钮，镜像进度与结果另起细行贴在下方。
 *
 * 状态与操作原先分属「知识库状态」「页面镜像」两个大面板，各占半屏；这里压到一行里——
 * 状态是背景信息，用户打开页面要的是目录与正文，不该先划过两块状态卡片。
 *
 * 镜像态（进度 / 本次结果 / 失败原因）仍留在这个组件里：长作业的中间态只影响工具区，
 * 不必上升成页面级状态（页面级只留抓取与打开文档）。
 */
export function Toolbar({
  api,
  status,
  report,
  mirror,
  refreshing,
  mcpOpen,
  embeddingOpen,
  onRefresh,
  onReveal,
  onToggleMcp,
  onToggleEmbedding,
  onChanged,
}: Props) {
  const [running, setRunning] = useState(false)
  const [progress, setProgress] = useState<MirrorProgress | null>(null)
  const [result, setResult] = useState<MirrorRun | null>(null)
  const [error, setError] = useState('')

  const start = async () => {
    setRunning(true)
    setError('')
    setResult(null)
    setProgress(null)
    try {
      setResult(
        await api.mirrorStart(null, (next) => {
          setProgress(next)
        }),
      )
      onChanged()
    } catch (cause) {
      setError(errorMessage(cause, t('common.failed')))
    } finally {
      setRunning(false)
    }
  }

  const stop = () => {
    void api.mirrorCancel().catch(() => undefined)
  }

  const days = wholeDays(status.age_days)
  const hasCatalog = status.cached_at !== null
  const jobRunning = running || (mirror?.running ?? false)
  // 本页发起的任务只看实时事件，避免启动瞬间回退到上一次作业的 `last`（会先闪出旧进度）。
  // 页面重载后则从后端状态恢复最近进度。
  const live = running ? progress : mirror?.running ? (mirror.last ?? null) : null
  const percent = live && live.total > 0 ? Math.round((live.done / live.total) * 100) : 0
  const mirrored = live?.mirrored ?? mirror?.mirrored ?? 0

  return (
    <section className="ka-toolbar">
      <div className="ka-toolbar-row">
        <div className="ka-toolbar-summary">
          {hasCatalog ? (
            <>
              <span className="ka-summary">
                {t('toolbar.docs', { count: status.docs })}
                {mirror && ` · ${t('mirror.mirrored', { mirrored, total: mirror.total })}`}
              </span>
              <span className="ka-meta">
                {t('toolbar.updated', { time: formatTime(status.cached_at) })}
                {days !== null && ` · ${t('toolbar.age', { days })}`}
                {status.stale && ` · ${t('toolbar.stale')}`}
                {mirror !== null && mirror.outdated > 0 && ` · ${t('mirror.outdated', { count: mirror.outdated })}`}
              </span>
            </>
          ) : (
            <span className="ka-hint">{status.error ?? t('toolbar.missing')}</span>
          )}
        </div>

        <div className="ka-actions">
          <button className="ka-primary" disabled={refreshing || jobRunning} onClick={onRefresh}>
            <RefreshCw aria-hidden />
            {refreshing ? t('toolbar.fetching') : hasCatalog ? t('toolbar.refresh') : t('toolbar.fetch')}
          </button>
          <button className="ka-ghost" onClick={onReveal}>
            <FolderOpen aria-hidden />
            {t('toolbar.openCacheDir')}
          </button>
          {hasCatalog &&
            (jobRunning ? (
              <button className="ka-ghost" onClick={stop}>
                <Square aria-hidden />
                {t('toolbar.cancel')}
              </button>
            ) : (
              <button className="ka-ghost" onClick={() => void start()}>
                <HardDriveDownload aria-hidden />
                {t('toolbar.mirror')}
              </button>
            ))}
          <button className="ka-ghost" aria-expanded={mcpOpen} onClick={onToggleMcp}>
            <Plug aria-hidden />
            {t('toolbar.mcp')}
          </button>
          <button className="ka-ghost" aria-expanded={embeddingOpen} onClick={onToggleEmbedding}>
            <BrainCircuit aria-hidden />
            {t('embedding.title')}
          </button>
        </div>
      </div>

      {report && (
        <p className="ka-meta">
          {t('toolbar.fetched', { documents: report.documents, bytes: formatBytes(report.bytes) })}
        </p>
      )}

      {jobRunning && (
        <div className="ka-progress" role="status">
          <div className="ka-progress-track">
            <div className="ka-progress-bar" style={{ width: `${percent}%` }} />
          </div>
          <span className="ka-meta">
            {live
              ? `${t('mirror.progress', { done: live.done, total: live.total })} · ${
                  live.skipped
                    ? `${live.title} · ${t('mirror.skipped')}`
                    : live.failed
                      ? `${live.title} · ${t('mirror.failed', { reason: live.failed })}`
                      : t('mirror.completed', { title: live.title })
                }`
              : t('mirror.preparing')}
          </span>
        </div>
      )}

      {result && (
        <p className="ka-meta">
          {t('mirror.done', { mirrored: result.mirrored, skipped: result.skipped, failed: result.failed })}
        </p>
      )}
      {error && (
        <div className="ka-error" role="alert">
          {error}
        </div>
      )}
    </section>
  )
}
