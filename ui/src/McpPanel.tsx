import { useEffect, useState } from 'react'
import type { KnowledgeStatus } from './types.generated'
import { Plug, X } from 'lucide-react'

import { copyText } from './clipboard'
import { t } from './i18n'
import type { MessageKey } from './i18n'
import { MCP_CLIENTS, mcpClientConfig, type McpClient } from './mcpGuides'

interface Props {
  status: KnowledgeStatus
  onClose: () => void
}

/**
 * MCP 接入：从右侧滑出的抽屉，里面是现成的客户端配置片段 + 一键复制 + 二进制检测。
 *
 * 做成抽屉而不是页面里的一块面板：接入配置是「偶尔看一眼、抄完就走」的东西，
 * 常驻会把正文挤矮，内联展开又会把三栏顶下去。抽屉盖在页面上，页面结构不动。
 *
 * ADR-010 定的是「只生成片段、不自动改写第三方配置」，所以这里没有"一键写入"，
 * 只有复制；能不能用由 `mcp_binary` 决定，界面直说。
 */
export function McpPanel({ status, onClose }: Props) {
  const [client, setClient] = useState<McpClient>('generic')
  const [copied, setCopied] = useState(false)
  const [promptCopied, setPromptCopied] = useState(false)
  const [error, setError] = useState('')
  const config = mcpClientConfig(client, status.mcp_binary)

  const key = (suffix: string) => `mcp.${client}.${suffix}` as MessageKey

  // Esc 关闭：抽屉盖住了整页，键盘用户不该只能去点遮罩。
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const copy = async () => {
    setError('')
    if (await copyText(config)) {
      setCopied(true)
      window.setTimeout(() => setCopied(false), 2000)
    } else {
      setError(t('mcp.copyFailed'))
    }
  }

  const copyPrompt = async () => {
    setError('')
    if (await copyText(t('mcp.testPrompt'))) {
      setPromptCopied(true)
      window.setTimeout(() => setPromptCopied(false), 2000)
    } else {
      setError(t('mcp.copyFailed'))
    }
  }

  return (
    <div className="ka-drawer-layer">
      <button className="ka-drawer-scrim" aria-label={t('common.close')} onClick={onClose} />

      <aside className="ka-drawer" role="dialog" aria-modal="true" aria-label={t('mcp.title')}>
        <header className="ka-drawer-head">
          <h2>
            <Plug aria-hidden />
            {t('mcp.title')}
          </h2>
          <div className="ka-actions">
            <button className="ka-ghost" onClick={onClose}>
              <X aria-hidden />
              {t('common.close')}
            </button>
          </div>
        </header>

        <div className="ka-drawer-body">
          <p className="ka-hint">{t('mcp.description')}</p>

          <div className="ka-client-tabs" role="tablist" aria-label={t('mcp.clientSelect')}>
            {MCP_CLIENTS.map((item) => (
              <button
                key={item}
                type="button"
                role="tab"
                aria-selected={client === item}
                className={client === item ? 'ka-client-tab ka-client-tab--active' : 'ka-client-tab'}
                onClick={() => {
                  setClient(item)
                  setCopied(false)
                  setError('')
                }}
              >
                {t(`mcp.client.${item}` as MessageKey)}
              </button>
            ))}
          </div>

          <section className="ka-guide" aria-labelledby="ka-guide-title">
            <h3 id="ka-guide-title">{t(key('title'))}</h3>
            <p className="ka-hint">{t(key('summary'))}</p>
            <ol className="ka-guide-steps">
              <li>{t(key('step1'))}</li>
              <li>{t(key('step2'))}</li>
              <li>{t(key('step3'))}</li>
            </ol>
          </section>

          <div className="ka-code-head">
            <strong>{t('mcp.configTitle')}</strong>
            <button className="ka-primary" onClick={() => void copy()}>
              {copied ? t('mcp.copied') : t('mcp.copy')}
            </button>
          </div>
          <pre className="ka-code">{config}</pre>
          <p className="ka-meta">
            {status.mcp_binary
              ? t('mcp.binaryPath', { path: status.mcp_binary })
              : t('mcp.binaryMissing', { name: 'mcp-knowledge.exe' })}
          </p>

          <section className="ka-guide ka-guide--test">
            <div className="ka-code-head">
              <strong>{t('mcp.verifyTitle')}</strong>
              <button className="ka-ghost" onClick={() => void copyPrompt()}>
                {promptCopied ? t('mcp.promptCopied') : t('mcp.copyPrompt')}
              </button>
            </div>
            <p className="ka-hint">{t('mcp.verifyDescription')}</p>
            <blockquote className="ka-prompt">{t('mcp.testPrompt')}</blockquote>
          </section>

          <p className="ka-hint">{t('mcp.readOnly')}</p>
          {error && (
            <div className="ka-error" role="alert">
              {error}
            </div>
          )}
        </div>
      </aside>
    </div>
  )
}
