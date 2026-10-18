import { useState } from 'react'
import { Drawer } from '@wonderland/ui'
import type { KnowledgeStatus } from './types.generated'

import { copyText } from './clipboard'
import { t } from './i18n'
import type { MessageKey } from './i18n'
import { MCP_CLIENTS, mcpClientConfig, type McpClient } from './mcpGuides'

interface Props {
  status: KnowledgeStatus
  onClose: () => void
}

/** Shared configuration dialog; copying never edits a third-party client's files. */
export function McpPanel({ status, onClose }: Props) {
  const [client, setClient] = useState<McpClient>('generic')
  const [copied, setCopied] = useState(false)
  const [promptCopied, setPromptCopied] = useState(false)
  const [error, setError] = useState('')
  const config = mcpClientConfig(client, status.mcp_binary)

  const key = (suffix: string) => `mcp.${client}.${suffix}` as MessageKey


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
    <Drawer open title={t('mcp.title')} onClose={onClose} closeLabel={t('common.close')} size="lg">
        <div className="ka-mcp-content">

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

          <details className="ka-guide"><summary>{t(key('title'))}</summary>
            <ol className="ka-guide-steps">
              <li>{t(key('step1'))}</li>
              <li>{t(key('step2'))}</li>
              <li>{t(key('step3'))}</li>
            </ol>
          </details>

          <div className="ka-code-head">
            <strong>{t('mcp.configTitle')}</strong>
            <button className="ka-primary" onClick={() => void copy()}>
              {copied ? t('mcp.copied') : t('mcp.copy')}
            </button>
          </div>
          <pre className="ka-code">{config}</pre>
          {!status.mcp_binary && <p className="ka-error" role="alert">{t('mcp.binaryMissing', { name: 'mcp-knowledge.exe' })}</p>}

          <section className="ka-guide ka-guide--test">
            <div className="ka-code-head">
              <strong>{t('mcp.verifyTitle')}</strong>
              <button className="ka-ghost" onClick={() => void copyPrompt()}>
                {promptCopied ? t('mcp.promptCopied') : t('mcp.copyPrompt')}
              </button>
            </div>

            <blockquote className="ka-prompt">{t('mcp.testPrompt')}</blockquote>
          </section>

          {error && (
            <div className="ka-error" role="alert">
              {error}
            </div>
          )}
        </div>
    </Drawer>
  )
}
