import { useEffect, useState } from 'react'
import { BrainCircuit, ExternalLink, FolderOpen, RefreshCw, X } from 'lucide-react'
import type { KnowledgeEmbeddingStatus } from './types.generated'

import type { KnowledgeApi } from './api'
import { errorMessage } from './display'
import { t } from './i18n'

interface Props {
  api: KnowledgeApi
  cacheDir: string
  onOpenCache: () => void
  onClose: () => void
}

/** 本地模型状态与安装引导；磁盘目录不等于运行时已经接入。 */
export function EmbeddingGuide({ api, cacheDir, onOpenCache, onClose }: Props) {
  const [error, setError] = useState('')
  const [status, setStatus] = useState<KnowledgeEmbeddingStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const separator = cacheDir.includes('\\') ? '\\' : '/'
  const modelDir = `${cacheDir.replace(/[\\/]$/, '')}${separator}models${separator}bge-small-zh-v1.5`

  useEffect(() => {
    let live = true
    void api.embeddingStatus().then(
      (value) => { if (live) setStatus(value) },
      (cause) => { if (live) setError(errorMessage(cause, t('common.failed'))) },
    )
    return () => { live = false }
  }, [api])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const openSource = async (source: 'onnx' | 'model') => {
    setError('')
    try {
      await api.openModelSource(source)
    } catch (cause) {
      setError(errorMessage(cause, t('common.failed')))
    }
  }

  const updateModel = async (operation: () => Promise<KnowledgeEmbeddingStatus>) => {
    setBusy(true)
    setError('')
    try {
      setStatus(await operation())
    } catch (cause) {
      setError(errorMessage(cause, t('common.failed')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="ka-drawer-layer ka-embedding-layer">
      <button className="ka-drawer-scrim" aria-label={t('common.close')} onClick={onClose} />
      <aside className="ka-drawer" role="dialog" aria-modal="true" aria-labelledby="ka-embedding-title">
        <header className="ka-drawer-head">
          <h2 id="ka-embedding-title">
            <BrainCircuit aria-hidden />
            {t('embedding.title')}
          </h2>
          <button className="ka-ghost" onClick={onClose}>
            <X aria-hidden />
            {t('common.close')}
          </button>
        </header>

        <div className="ka-drawer-body ka-embedding-body">
          <p className="ka-hint">{t('embedding.summary')}</p>

          <section className="ka-guide ka-embedding-status" aria-labelledby="ka-embedding-current">
            <div className="ka-embedding-status-head">
              <h3 id="ka-embedding-current">{t('embedding.currentTitle')}</h3>
              <button className="ka-ghost" disabled={busy} onClick={() => void updateModel(() => api.recheckEmbeddings())}>
                <RefreshCw aria-hidden /> {t('embedding.recheck')}
              </button>
            </div>
            {status ? (
              <>
                <div className="ka-embedding-switch">
                  <div>
                    <strong>{status.active ? t('embedding.loaded') : t('embedding.manualOnly')}</strong>
                    <p className="ka-meta">{status.active ? t('embedding.active') : t('embedding.inactive')}</p>
                  </div>
                  <button
                    className={status.active ? 'ka-ghost' : 'ka-primary'}
                    disabled={busy || (!status.active && status.models[0]?.missing_files.length !== 0)}
                    onClick={() => void updateModel(() => status.active ? api.disableEmbeddings() : api.activateEmbeddings())}
                  >
                    {status.active ? t('embedding.disable') : t('embedding.loadAndBuild')}
                  </button>
                </div>
                {status.error && <div className="ka-error" role="status">{status.error}</div>}
                <div className="ka-embedding-models">
                  {status.models.map((model) => {
                    const installed = model.supported && model.missing_files.length === 0
                    const modelState = !model.supported
                      ? t('embedding.unsupported')
                      : !installed
                        ? t('embedding.incomplete')
                        : status.active
                          ? t('embedding.loaded')
                          : t('embedding.ready')
                    return (
                      <div className="ka-embedding-model" key={model.path}>
                        <div className="ka-embedding-model-head">
                          <strong>{model.name}</strong>
                          <span className="ka-embedding-badge">{modelState}</span>
                        </div>
                        <code className="ka-embedding-path">{model.path}</code>
                        {model.supported && model.missing_files.length > 0 && (
                          <p className="ka-meta">{t('embedding.missingFiles')}: {model.missing_files.join('、')}</p>
                        )}
                      </div>
                    )
                  })}
                </div>
                <p className="ka-meta">{t('embedding.manualNotice')}</p>
              </>
            ) : <p className="ka-meta">{t('embedding.loading')}</p>}
          </section>

          <section className="ka-guide" aria-labelledby="ka-embedding-needs">
            <h3 id="ka-embedding-needs">{t('embedding.requiredTitle')}</h3>
            <ul className="ka-embedding-list">
              <li>{t('embedding.requiredLocal')}</li>
              <li>{t('embedding.requiredChinese')}</li>
              <li>{t('embedding.requiredCompatible')}</li>
            </ul>
          </section>

          <section className="ka-guide ka-embedding-recommend" aria-labelledby="ka-embedding-recommendation">
            <h3 id="ka-embedding-recommendation">{t('embedding.recommendedTitle')}</h3>
            <p className="ka-hint">{t('embedding.recommendedText')}</p>
            <div className="ka-embedding-links">
              <button className="ka-ghost" onClick={() => void openSource('onnx')}>
                {t('embedding.onnxSource')} <ExternalLink aria-hidden />
              </button>
              <button className="ka-ghost" onClick={() => void openSource('model')}>
                {t('embedding.modelSource')} <ExternalLink aria-hidden />
              </button>
            </div>
          </section>

          <details className="ka-embedding-details">
            <summary>{t('embedding.presetDetails')}</summary>
            <p className="ka-meta">{t('embedding.presetNote')}</p>
            <code className="ka-embedding-path">{modelDir}</code>
            <ul className="ka-embedding-files">
              <li>model_optimized.onnx</li>
              <li>tokenizer.json</li>
              <li>config.json</li>
              <li>special_tokens_map.json</li>
              <li>tokenizer_config.json</li>
            </ul>
          </details>

          <p className="ka-meta">{t('embedding.fallback')}</p>
          <button className="ka-primary ka-embedding-cache" onClick={onOpenCache}>
            <FolderOpen aria-hidden />
            {t('toolbar.openCacheDir')}
          </button>
          {error && <div className="ka-error" role="alert">{error}</div>}
        </div>
      </aside>
    </div>
  )
}
