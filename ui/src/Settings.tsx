import { useRef, useState } from 'react'
import { Drawer, confirmDialog } from '@wonderland/ui'
import { Copy, Plus, Trash2, Pencil } from 'lucide-react'
import { copyText } from './clipboard'
import { mcpClientConfig, type McpClient } from './mcpGuides'
import type { Api, Model, Service, Status } from './workspaceTypes'

type Action = (method: string, params?: Record<string, unknown>) => Promise<unknown>
const freshService = (): Service => ({
  id: '',
  name: '',
  kind: 'ollama',
  url: 'http://127.0.0.1:11434',
})
const freshModel = (service = ''): Model => ({
  id: '',
  name: '',
  service_id: service,
  model: '',
  dimensions: 0,
  max_chars: 1000,
  query_prefix: '',
  document_prefix: '',
})
function modelDraft(model: Model): Model {
  return {
    id: model.id,
    name: model.name,
    service_id: model.service_id,
    model: model.model,
    dimensions: 0,
    max_chars: model.max_chars,
    query_prefix: model.query_prefix,
    document_prefix: model.document_prefix,
  }
}

export function Settings({
  api,
  status,
  onClose,
  action,
  report,
}: {
  api: Api
  status: Status
  onClose: () => void
  onChanged: () => Promise<void>
  action: Action
  report: (message: string) => void
}) {
  const [section, setSection] = useState('models')
  const [service, setService] = useState<Service | null>(null)
  const [key, setKey] = useState('')
  const [clearKey, setClearKey] = useState(false)
  const [profile, setProfile] = useState<Model | null>(null)
  const [models, setModels] = useState<string[]>([])
  const listEpoch = useRef(0)
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<{ dimensions: number; similarities: number[] } | null>(null)
  const [texts, setTexts] = useState(
    '如何取消采集并保留成功快照？\nCancel the collection while retaining the successful snapshot.\nThe weather is sunny today.',
  )
  const [client, setClient] = useState<McpClient>('generic')
  const [copied, setCopied] = useState(false)
  const running = status.job.state === 'running'
  const mcp = status.mcp_config.mcpServers.wla_knowledge_library
  const config = mcpClientConfig(client, mcp.command, status.cache_dir)
  const invoke = async (method: string, params?: Record<string, unknown>) => {
    setBusy(true)
    try {
      return await action(method, params)
    } finally {
      setBusy(false)
    }
  }
  const editService = (value: Service) => {
    setService(value)
    setProfile(null)
    setKey('')
    setClearKey(false)
  }
  const editModel = async (value: Model) => {
    const epoch = ++listEpoch.current
    setProfile(value)
    setService(null)
    setModels([])
    setResult(null)
    try {
      const names = await api.call<string[]>('service_models', { service_id: value.service_id })
      if (epoch === listEpoch.current) setModels(names)
    } catch (reason) {
      if (epoch === listEpoch.current) report(`${String(reason)}；仍可手填模型标识`)
    }
  }
  const deleteItem = async (kind: 'service' | 'model', id: string) => {
    if (
      !(await confirmDialog({
        title: kind === 'service' ? '移除模型服务' : '移除模型',
        message: '相关库将解除绑定并需要重建语义索引，正文与关键词索引保留。',
        confirmLabel: '移除',
        danger: true,
      }))
    )
      return
    await invoke(`${kind}_delete`, { [`${kind}_id`]: id, confirmed: true })
  }
  return (
    <Drawer open title="全局设置" onClose={onClose} closeLabel="关闭" size="lg">
      <div className="kb-settings">
        <nav className="kb-settings-nav" aria-label="设置分区">
          {[
            ['models', '模型服务'],
            ['mcp', 'MCP'],
            ['presets', '预设'],
            ['local', '本机'],
          ].map(([id, name]) => (
            <button
              key={id}
              aria-current={section === id ? 'page' : undefined}
              className={section === id ? 'active' : ''}
              onClick={() => setSection(id)}
            >
              {name}
            </button>
          ))}
        </nav>
        <div className="kb-settings-content">
          {section === 'models' ? (
            <>
              {service ? (
                <section className="kb-form">
                  <h3>{service.id ? '编辑服务' : '添加服务'}</h3>
                  <label>
                    名称
                    <input
                      autoFocus
                      value={service.name}
                      maxLength={128}
                      onChange={(event) => setService({ ...service, name: event.target.value })}
                    />
                  </label>
                  <label>
                    接口类型
                    <select
                      value={service.kind}
                      onChange={(event) =>
                        setService({
                          ...service,
                          kind: event.target.value as Service['kind'],
                          url:
                            event.target.value === 'ollama'
                              ? 'http://127.0.0.1:11434'
                              : 'http://127.0.0.1:1234/v1',
                        })
                      }
                    >
                      <option value="ollama">Ollama</option>
                      <option value="openai">OpenAI 兼容</option>
                    </select>
                  </label>
                  <label>
                    本机服务地址
                    <input
                      value={service.url}
                      onChange={(event) => setService({ ...service, url: event.target.value })}
                      spellCheck={false}
                    />
                  </label>
                  <details>
                    <summary>认证</summary>
                    <label>
                      API Key
                      <input
                        type="password"
                        autoComplete="new-password"
                        value={key}
                        onChange={(event) => setKey(event.target.value)}
                        placeholder={service.secret_id ? '已配置；留空保留' : '可选'}
                      />
                    </label>
                    {service.secret_id ? (
                      <label className="kb-check">
                        <input
                          type="checkbox"
                          checked={clearKey}
                          onChange={(event) => setClearKey(event.target.checked)}
                        />
                        清除已保存认证
                      </label>
                    ) : null}
                  </details>
                  <div className="kb-actions">
                    <button onClick={() => setService(null)}>返回</button>
                    <button
                      className="kb-primary"
                      disabled={busy || running || !service.name.trim()}
                      onClick={() =>
                        void invoke('service_save', {
                          service: {
                            id: service.id,
                            name: service.name,
                            kind: service.kind,
                            url: service.url,
                          },
                          ...(key ? { api_key: key } : {}),
                          clear_key: clearKey,
                        }).then((value) => {
                          if (value) {
                            setService(null)
                            setKey('')
                          }
                        })
                      }
                    >
                      保存服务
                    </button>
                  </div>
                </section>
              ) : profile ? (
                <section className="kb-form">
                  <h3>{profile.id ? '重新验证模型' : '连接模型'}</h3>
                  <label>
                    服务
                    <select
                      value={profile.service_id}
                      onChange={(event) =>
                        void editModel({ ...profile, service_id: event.target.value })
                      }
                    >
                      {status.services.map((value) => (
                        <option key={value.id} value={value.id}>
                          {value.name}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    模型标识
                    <input
                      list="kb-model-options"
                      value={profile.model}
                      maxLength={256}
                      spellCheck={false}
                      onChange={(event) => {
                        setResult(null)
                        setProfile({ ...profile, model: event.target.value })
                      }}
                    />
                    <datalist id="kb-model-options">
                      {models.map((name) => (
                        <option key={name} value={name} />
                      ))}
                    </datalist>
                  </label>
                  <label>
                    显示名称
                    <input
                      value={profile.name}
                      maxLength={128}
                      onChange={(event) => setProfile({ ...profile, name: event.target.value })}
                    />
                  </label>
                  <details>
                    <summary>输入适配</summary>
                    <div className="kb-form">
                      <label>
                        单次输入上限（字符，含前缀）
                        <input
                          type="number"
                          min={64}
                          max={16000}
                          value={profile.max_chars}
                          onChange={(event) => {
                            setResult(null)
                            setProfile({ ...profile, max_chars: Number(event.target.value) })
                          }}
                        />
                      </label>
                      <label>
                        查询前缀
                        <input
                          value={profile.query_prefix}
                          onChange={(event) => {
                            setResult(null)
                            setProfile({ ...profile, query_prefix: event.target.value })
                          }}
                        />
                      </label>
                      <label>
                        正文前缀
                        <input
                          value={profile.document_prefix}
                          onChange={(event) => {
                            setResult(null)
                            setProfile({ ...profile, document_prefix: event.target.value })
                          }}
                        />
                      </label>
                    </div>
                  </details>
                  <details>
                    <summary>中英文与跨语言自测</summary>
                    <label>
                      首行为查询，其余为对照资料
                      <textarea
                        rows={6}
                        value={texts}
                        onChange={(event) => {
                          setTexts(event.target.value)
                          setResult(null)
                        }}
                      />
                    </label>
                    <p className="kb-help">
                      比较相似度用于自测；请用与你的资料相近的文本验证效果。
                    </p>
                  </details>
                  {result ? (
                    <div className="kb-test-result" role="status">
                      <strong>嵌入有效 · {result.dimensions} 维</strong>
                      {result.similarities.map((score, index) => (
                        <p key={index}>
                          文本 {index + 1}：{score.toFixed(4)}
                        </p>
                      ))}
                    </div>
                  ) : null}
                  <div className="kb-actions">
                    <button onClick={() => setProfile(null)}>返回</button>
                    <button
                      disabled={busy || running || !profile.model.trim()}
                      onClick={() =>
                        void invoke('model_test', {
                          profile: { ...profile, name: profile.name || profile.model },
                          texts: texts.split('\n').filter((text) => text.trim()),
                        }).then((value) => {
                          if (value) setResult(value as typeof result)
                        })
                      }
                    >
                      {busy ? '验证中…' : '验证与自测'}
                    </button>
                    <button
                      className="kb-primary"
                      disabled={busy || running || !profile.model.trim()}
                      onClick={() =>
                        void invoke('model_save', {
                          profile: { ...profile, name: profile.name || profile.model },
                        }).then((value) => {
                          if (value) setProfile(null)
                        })
                      }
                    >
                      验证并保存
                    </button>
                  </div>
                </section>
              ) : (
                <>
                  <div className="kb-section-heading">
                    <h3>本机服务</h3>
                    <button disabled={running} onClick={() => editService(freshService())}>
                      <Plus size={14} />
                      添加
                    </button>
                  </div>
                  {status.services.map((value) => (
                    <div className="kb-setting-item" key={value.id}>
                      <div>
                        <strong>{value.name}</strong>
                        <small>
                          {value.kind === 'ollama' ? 'Ollama' : 'OpenAI 兼容'} · {value.url}
                        </small>
                      </div>
                      <button
                        className="kb-icon"
                        aria-label={`编辑 ${value.name}`}
                        disabled={running}
                        onClick={() => editService(value)}
                      >
                        <Pencil size={14} />
                      </button>
                      <button
                        className="kb-icon kb-danger"
                        aria-label={`移除 ${value.name}`}
                        disabled={busy || running}
                        onClick={() => void deleteItem('service', value.id)}
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  ))}
                  {!status.services.length ? (
                    <p className="kb-help">尚未连接本机嵌入服务。</p>
                  ) : null}
                  <div className="kb-section-heading">
                    <h3>已验证模型</h3>
                    <button
                      disabled={running || !status.services.length}
                      onClick={() => void editModel(freshModel(status.services[0]?.id))}
                    >
                      <Plus size={14} />
                      连接模型
                    </button>
                  </div>
                  {status.models.map((value) => (
                    <div className="kb-setting-item" key={value.id}>
                      <div>
                        <strong>{value.name}</strong>
                        <small>
                          {status.services.find((service) => service.id === value.service_id)?.name}{' '}
                          · {value.dimensions} 维
                        </small>
                        <small>{value.model}</small>
                      </div>
                      <button
                        className="kb-icon"
                        aria-label={`验证 ${value.name}`}
                        disabled={running}
                        onClick={() => void editModel(modelDraft(value))}
                      >
                        <Pencil size={14} />
                      </button>
                      <button
                        className="kb-icon kb-danger"
                        aria-label={`移除 ${value.name}`}
                        disabled={busy || running}
                        onClick={() => void deleteItem('model', value.id)}
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  ))}
                  <details>
                    <summary>模型要求</summary>
                    <p className="kb-help">
                      由你选择并在本机运行文本嵌入模型。需要稳定维度、有效非零向量；多语言和跨语言能力由模型决定。支持自动读取列表或手填标识，保存前将实际请求验证。输入上限与前缀请按模型说明设置；长文分段计算，不静默截断。模型在外部升级后，请重新验证并重建索引。
                    </p>
                  </details>
                </>
              )}
            </>
          ) : section === 'mcp' ? (
            <section className="kb-form">
              <h3>可访问的知识库</h3>
              {status.libraries.map((library) => (
                <label key={library.id} className="kb-check">
                  <input
                    type="checkbox"
                    disabled={busy || running}
                    checked={library.exposed}
                    onChange={(event) =>
                      void invoke('library_update', {
                        library_id: library.id,
                        exposed: event.target.checked,
                      })
                    }
                  />
                  {library.name}
                </label>
              ))}
              <details>
                <summary>访问范围说明</summary>
                <p className="kb-help">
                  只读访问勾选的库；未指定库时检索全部已授权库，新库默认关闭。
                </p>
              </details>
              <label>
                客户端格式
                <select
                  value={client}
                  onChange={(event) => {
                    setClient(event.target.value as McpClient)
                    setCopied(false)
                  }}
                >
                  <option value="generic">通用 JSON</option>
                  <option value="codex">Codex TOML</option>
                  <option value="workbuddy">WorkBuddy</option>
                  <option value="trae">Trae</option>
                </select>
              </label>
              <pre className="kb-config">{config}</pre>
              <button
                onClick={() =>
                  void copyText(config).then((ok) => {
                    setCopied(ok)
                    if (!ok) report('复制失败，请手动选择配置文本')
                  })
                }
              >
                <Copy size={14} />
                {copied ? '已复制配置' : '复制配置'}
              </button>
            </section>
          ) : section === 'presets' ? (
            <section>
              <div className="kb-section-heading">
                <h3>预设配方</h3>
                <button disabled={busy || running} onClick={() => void invoke('preset_import')}>
                  导入预设
                </button>
              </div>
              {status.presets.map((preset) => (
                <div className="kb-setting-item" key={preset.id}>
                  <span>{preset.name}</span>
                  <button
                    className="kb-link"
                    onClick={() => void invoke('preset_export', { preset_id: preset.id })}
                  >
                    导出
                  </button>
                </div>
              ))}
              <details>
                <summary>预设内容</summary>
                <p className="kb-help">保存来源配置和规则，正文、索引和模型独立管理。</p>
              </details>
            </section>
          ) : (
            <section className="kb-form">
              <h3>本机运行</h3>
              <p>Microsoft Edge：{status.edge_available ? '可用' : '未发现，动态采集需要安装'}</p>
              <label>
                资料目录<span className="kb-path">{status.cache_dir}</span>
              </label>
              <p className="kb-help">知识库 {status.version}</p>
            </section>
          )}
        </div>
      </div>
    </Drawer>
  )
}
