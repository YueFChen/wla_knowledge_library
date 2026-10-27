import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Drawer, confirmDialog } from '@wonderland/ui'
import {
  BookOpen,
  ChevronLeft,
  Database,
  Download,
  FolderOpen,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  Trash2,
  X,
} from 'lucide-react'
import { Reader } from './Reader'
import { documentUrlKey } from './readingLinks'
import { DocumentTree } from './DocumentTree'
import { TaskLogPanel } from './TaskLogPanel'
import { LibrarySettings } from './LibrarySettings'
import { SourceEditor } from './SourceEditor'
import { Settings } from './Settings'
import type { Api, Brief, Hit, Source, Status } from './workspaceTypes'

type Editing = {
  mode: 'create' | 'rename' | 'preset'
  name: string
  preset: string
  libraryId?: string
}
type Reading = { libraryId: string; documentId: string; anchor?: string }
const formats = {
  directory: '本地目录',
  files: '本地文件',
  website: '网页资料',
  wonderland: '千星预设',
}

export function WorkspacePage({ api }: { api: Api }) {
  const [status, setStatus] = useState<Status | null>(null)
  const [selected, setSelected] = useState('')
  const [tab, setTab] = useState<'search' | 'sources'>('search')
  const [settings, setSettings] = useState(false)
  const [librarySettings, setLibrarySettings] = useState(false)
  const [editing, setEditing] = useState<Editing | null>(null)
  const [editor, setEditor] = useState<{ libraryId: string; source?: Source } | null>(null)
  const [documents, setDocuments] = useState<Brief[]>([])
  const [query, setQuery] = useState('')
  const [scope, setScope] = useState<string[]>([])
  const [hits, setHits] = useState<Hit[] | null>(null)
  const [warnings, setWarnings] = useState<string[]>([])
  const [reading, setReading] = useState<Reading | null>(null)
  const [searching, setSearching] = useState(false)
  const [pending, setPending] = useState(0)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  const resultsRef = useRef<HTMLDivElement>(null)
  const resultsScroll = useRef(0)
  useLayoutEffect(() => {
    if (!reading && resultsRef.current) resultsRef.current.scrollTop = resultsScroll.current
  }, [reading])
  const statusEpoch = useRef(0)
  const searchEpoch = useRef(0)
  const library = status?.libraries.find((value) => value.id === selected)
  const running = status?.job.state === 'running'
  const report = useCallback((reason: unknown) => setError(String(reason)), [])
  const refresh = useCallback(async () => {
    const epoch = ++statusEpoch.current
    const next = await api.call<Status>('status')
    if (epoch === statusEpoch.current) setStatus(next)
  }, [api])
  useEffect(() => {
    void refresh().catch(report)
  }, [refresh, report])
  const polling = pending > 0 || running
  useEffect(() => {
    if (!polling) return
    const timer = window.setInterval(() => {
      void refresh().catch(report)
    }, 900)
    return () => window.clearInterval(timer)
  }, [polling, refresh, report])
  useEffect(() => {
    let active = true
    if (!selected) {
      setDocuments([])
      return
    }
    void api
      .call<Brief[]>('documents', { library_id: selected })
      .then((value) => {
        if (active) setDocuments(value)
      })
      .catch(report)
    return () => {
      active = false
    }
  }, [api, selected, library?.revision, report])
  const select = (id: string) => {
    ++searchEpoch.current
    setSearching(false)
    setSelected(id)
    setScope(id ? [id] : [])
    setTab('search')
    setQuery('')
    setHits(null)
    setReading(null)
    setWarnings([])
    resultsScroll.current = 0
    setLibrarySettings(false)
  }
  const action = async (method: string, params?: Record<string, unknown>) => {
    setPending((value) => value + 1)
    setError('')
    setMessage('')
    try {
      const result = await api.call(method, params)
      if (method.endsWith('_export')) setMessage('已保存至 Core 导出目录')
      return result
    } catch (reason) {
      report(reason)
      return undefined
    } finally {
      await refresh().catch(() => {})
      setPending((value) => value - 1)
    }
  }
  const search = async () => {
    if (!query.trim() || !scope.length) return
    const epoch = ++searchEpoch.current
    setSearching(true)
    setError('')
    setWarnings([])
    try {
      const result = await api.call<{ hits: Hit[]; warnings: string[] }>('search', {
        library_ids: scope,
        query: query.trim(),
        top_k: 30,
      })
      if (epoch === searchEpoch.current) {
        resultsScroll.current = 0
        setHits(result.hits)
        setWarnings(result.warnings)
        setReading(null)
      }
    } catch (reason) {
      if (epoch === searchEpoch.current) report(reason)
    } finally {
      if (epoch === searchEpoch.current) setSearching(false)
    }
  }
  const saveName = async () => {
    if (!editing?.name.trim()) return
    const result = await action(
      editing.mode === 'create'
        ? 'library_create'
        : editing.mode === 'rename'
          ? 'library_update'
          : 'preset_save',
      editing.mode === 'create'
        ? { name: editing.name, preset_id: editing.preset }
        : { library_id: editing.libraryId, name: editing.name },
    )
    if (result) {
      setEditing(null)
      if (
        editing.mode === 'create' &&
        typeof result === 'object' &&
        'id' in result &&
        typeof result.id === 'string'
      )
        select(result.id)
    }
  }
  const remove = async (sourceId?: string) => {
    if (!library) return
    const target = sourceId
      ? library.sources.find((value) => value.id === sourceId)?.name
      : library.name
    const affected = sourceId
      ? documents.filter((document) => document.source_id === sourceId).length
      : documents.length
    if (
      !(await confirmDialog({
        title: sourceId ? '永久删除资料源' : '永久删除知识库',
        message: `将永久清理“${target}”对应的 ${affected} 篇库内资料、快照和索引。外部源文件保持原样。`,
        confirmLabel: '永久删除',
        danger: true,
      }))
    )
      return
    const result = await action('delete', {
      library_id: selected,
      ...(sourceId ? { source_id: sourceId } : {}),
      confirmed: true,
    })
    if (result) {
      setReading(null)
      setHits(null)
      if (!sourceId) select('')
    }
  }
  const libraryName = reading
    ? status?.libraries.find((value) => value.id === reading.libraryId)?.name
    : ''
  return (
    <div className="kb-app">
      {error ? (
        <div className="kb-notice kb-error" role="alert">
          <span>{error}</span>
          <button className="kb-icon" aria-label="关闭错误" onClick={() => setError('')}>
            <X size={16} />
          </button>
        </div>
      ) : message ? (
        <div className="kb-notice" role="status">
          {message}
          <button className="kb-icon" aria-label="关闭消息" onClick={() => setMessage('')}>
            <X size={16} />
          </button>
        </div>
      ) : null}
      {!status ? (
        <div className="kb-empty">
          <Database size={36} />
          <h2>正在连接知识库</h2>
          <button onClick={() => void refresh().catch(report)}>重新连接</button>
        </div>
      ) : !library ? (
        <main className="kb-library-home">
          <div className="kb-home-heading">
            <div>
              <h1>我的知识库</h1>
            </div>
            <div className="kb-actions">
              <button className="kb-icon" aria-label="知识库设置" onClick={() => setSettings(true)}>
                <Settings2 size={19} />
              </button>
              <button disabled={!!pending || running} onClick={() => void action('library_import')}>
                <FolderOpen size={16} />
                导入备份
              </button>
              <button
                className="kb-primary"
                onClick={() => setEditing({ mode: 'create', name: '', preset: 'blank' })}
              >
                <Plus size={16} />
                创建知识库
              </button>
            </div>
          </div>
          {status.libraries.length ? (
            <div className="kb-library-grid">
              {status.libraries.map((value) => (
                <button className="kb-library-card" key={value.id} onClick={() => select(value.id)}>
                  <span className="kb-card-icon">
                    <BookOpen size={22} />
                  </span>
                  <h2>{value.name}</h2>
                  <p>
                    {status.presets.find((preset) => preset.id === value.preset_id)?.name ||
                      '自定义知识库'}
                  </p>
                  <div>
                    <span>{value.sources.length} 个资料源</span>
                    <span>{value.snapshot ? '已构建' : '待构建'}</span>
                  </div>
                  <footer>
                    <span>
                      {value.model && status.models.some((model) => model.id === value.model)
                        ? value.vector_revision
                          ? '语义已构建'
                          : '语义待构建'
                        : '关键词检索'}
                    </span>
                    <span>{value.exposed ? 'MCP 可读' : '本地使用'}</span>
                  </footer>
                </button>
              ))}
            </div>
          ) : (
            <section className="kb-empty kb-welcome">
              <BookOpen size={42} />
              <h2>建立你的第一座资料库</h2>
              <button
                className="kb-primary"
                onClick={() => setEditing({ mode: 'create', name: '', preset: 'wonderland' })}
              >
                从千星预设开始
              </button>
            </section>
          )}
        </main>
      ) : (
        <main className="kb-workbench">
          <div className="kb-library-heading">
            <div>
              <button className="kb-back" onClick={() => select('')}>
                <ChevronLeft size={14} />
                所有知识库
              </button>
              <h1>{library.name}</h1>
              <div className="kb-metadata">
                <span>{documents.length} 篇资料</span>
                <span>
                  {library.model
                    ? library.vector_revision
                      ? '关键词 + 语义'
                      : '语义待构建'
                    : '关键词检索'}
                </span>
                <span>{library.exposed ? 'MCP 已暴露' : 'MCP 未暴露'}</span>
              </div>
            </div>
            <div className="kb-actions">
              <button
                className="kb-primary"
                disabled={!!pending || running || !library.sources.length}
                onClick={() => void action('build', { library_id: selected })}
              >
                <RefreshCw size={15} />
                更新资料
              </button>
              <button
                className="kb-icon"
                aria-label="当前库设置"
                onClick={() => setLibrarySettings(true)}
              >
                <Settings2 size={19} />
              </button>
              <details className="kb-menu">
                <summary aria-label="知识库更多操作">
                  <MoreHorizontal size={19} />
                </summary>
                <div>
                  <button onClick={() => setSettings(true)}>全局设置</button>
                  <button
                    onClick={() =>
                      setEditing({
                        mode: 'rename',
                        name: library.name,
                        preset: '',
                        libraryId: library.id,
                      })
                    }
                  >
                    重命名
                  </button>
                  <button
                    disabled={running}
                    onClick={() => void action('library_export', { library_id: selected })}
                  >
                    <Download size={14} />
                    导出备份
                  </button>
                  <button
                    onClick={() =>
                      setEditing({
                        mode: 'preset',
                        name: `${library.name}配方`,
                        preset: '',
                        libraryId: library.id,
                      })
                    }
                  >
                    保存为预设
                  </button>
                  <button className="kb-danger" disabled={running} onClick={() => void remove()}>
                    <Trash2 size={14} />
                    删除知识库
                  </button>
                </div>
              </details>
            </div>
          </div>
          <nav className="kb-tabs" aria-label="库工作台">
            <button className={tab === 'search' ? 'active' : ''} onClick={() => setTab('search')}>
              <Search size={15} />
              检索与阅读
            </button>
            <button className={tab === 'sources' ? 'active' : ''} onClick={() => setTab('sources')}>
              <Database size={15} />
              资料源<span>{library.sources.length}</span>
            </button>
          </nav>
          {tab === 'sources' ? (
            <div className="kb-sources">
              <div className="kb-section-heading">
                <div>
                  <h2>资料来源</h2>
                </div>
                <button disabled={running} onClick={() => setEditor({ libraryId: selected })}>
                  <Plus size={15} />
                  添加资料源
                </button>
              </div>
              {library.sources.map((value) => (
                <div className="kb-source" key={value.id}>
                  <div>
                    <span className="kb-badge">{formats[value.kind]}</span>
                    <h3>{value.name}</h3>
                    <p className="kb-path">
                      {value.kind === 'website'
                        ? value.url
                        : value.kind === 'wonderland'
                          ? '官方目录与正文 · 节点和 Lua 扩展'
                          : value.paths.length
                            ? value.paths.join(' · ')
                            : '请重新选择来源路径'}
                    </p>
                  </div>
                  <div className="kb-actions">
                    <button
                      disabled={running}
                      onClick={() => setEditor({ libraryId: selected, source: value })}
                    >
                      编辑
                    </button>
                    <button
                      className="kb-icon kb-danger"
                      disabled={running}
                      aria-label={`删除资料源 ${value.name}`}
                      onClick={() => void remove(value.id)}
                    >
                      <Trash2 size={16} />
                    </button>
                  </div>
                </div>
              ))}
              {!library.sources.length ? (
                <div className="kb-empty">
                  <FolderOpen size={32} />
                  <p>添加文件、目录或网页后开始构建。</p>
                </div>
              ) : null}
            </div>
          ) : (
            <div className="kb-reading-workspace">
              <form
                className="kb-query"
                onSubmit={(event) => {
                  event.preventDefault()
                  void search()
                }}
              >
                <Search size={18} />
                <input
                  aria-label="检索资料"
                  placeholder="搜索资料、API 或描述问题…"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                />
                <button
                  className="kb-primary"
                  disabled={searching || !query.trim() || !scope.length}
                >
                  {searching ? '检索中…' : '检索'}
                </button>
              </form>
              <div className="kb-search-options">
                <details>
                  <summary>
                    检索范围 ·{' '}
                    {scope.length === 1 && scope[0] === selected
                      ? '当前库'
                      : `${scope.length} 个库`}
                  </summary>
                  <div>
                    {status.libraries.map((value) => (
                      <label className="kb-check" key={value.id}>
                        <input
                          type="checkbox"
                          checked={scope.includes(value.id)}
                          onChange={(event) => {
                            ++searchEpoch.current
                            setSearching(false)
                            setHits(null)
                            setScope((current) =>
                              event.target.checked
                                ? [...current, value.id]
                                : current.filter((id) => id !== value.id),
                            )
                          }}
                        />
                        {value.name}
                      </label>
                    ))}
                  </div>
                </details>
                <span>{documents.length} 篇资料</span>
              </div>
              <div className={`kb-reading-grid${reading ? '' : ' kb-no-outline'}`}>
                <DocumentTree
                  documents={documents}
                  sources={library.sources}
                  active={reading?.libraryId === selected ? reading.documentId : undefined}
                  open={(id) => setReading({ libraryId: selected, documentId: id })}
                />
                {reading && libraryName ? (
                  <Reader
                    key={`${reading.libraryId}:${reading.documentId}:${status.libraries.find((value) => value.id === reading.libraryId)?.revision}`}
                    api={api}
                    libraryId={reading.libraryId}
                    documentId={reading.documentId}
                    anchor={reading.anchor}
                    onClose={() => setReading(null)}
                    onError={report}
                    hasResults={hits !== null}
                    onLink={(url) => {
                      const target = new URL(url)
                      const targetKey = documentUrlKey(target.href)
                      const found = targetKey
                        ? documents.find((document) => documentUrlKey(document.url) === targetKey)
                        : undefined
                      if (!found) return false
                      setReading({
                        libraryId: selected,
                        documentId: found.id,
                        anchor: decodeURIComponent(target.hash.slice(1)) || undefined,
                      })
                      return true
                    }}
                  />
                ) : (
                  <section
                    className="kb-results-pane"
                    ref={resultsRef}
                    onScroll={(event) => {
                      resultsScroll.current = event.currentTarget.scrollTop
                    }}
                  >
                    {warnings.map((warning, index) => (
                      <p className="kb-warning" key={index}>
                        {warning}
                      </p>
                    ))}
                    {searching ? (
                      <p className="kb-help" role="status">
                        正在检索…
                      </p>
                    ) : null}
                    {hits ? (
                      <>
                        <div className="kb-section-heading">
                          <span>{hits.length} 条结果</span>
                          <button
                            className="kb-link"
                            onClick={() => {
                              setHits(null)
                              setWarnings([])
                            }}
                          >
                            清除结果
                          </button>
                        </div>
                        {hits.map((hit, index) => (
                          <button
                            className="kb-result"
                            key={`${hit.library_id}:${hit.path_id}:${index}`}
                            onClick={() =>
                              setReading({
                                libraryId: hit.library_id,
                                documentId: hit.path_id,
                                anchor: hit.anchor,
                              })
                            }
                          >
                            <small>
                              {hit.library_name} · {hit.breadcrumb.slice(0, -1).join(' / ')}
                            </small>
                            <h3>{hit.doc_title}</h3>
                            <p>{hit.text.slice(0, 220)}</p>
                          </button>
                        ))}
                        {!hits.length ? (
                          <div className="kb-empty">
                            <Search size={28} />
                            <p>没有匹配结果</p>
                          </div>
                        ) : null}
                      </>
                    ) : (
                      <div className="kb-empty">
                        <BookOpen size={32} />
                        <p>
                          {documents.length ? '从目录选择资料，或输入关键词检索' : '资料尚未构建'}
                        </p>
                        {!documents.length ? (
                          <button onClick={() => setTab('sources')}>管理资料源</button>
                        ) : null}
                      </div>
                    )}
                  </section>
                )}
              </div>
            </div>
          )}
        </main>
      )}
      {status?.job.id ? (
        <TaskLogPanel api={api} job={status.job} cancel={() => void action('job_cancel')} />
      ) : null}
      {librarySettings && library && status ? (
        <LibrarySettings
          library={library}
          status={status}
          close={() => setLibrarySettings(false)}
          global={() => {
            setLibrarySettings(false)
            setSettings(true)
          }}
          action={action}
          busy={!!pending || running}
        />
      ) : null}
      {editing ? (
        <Drawer
          open
          title={
            editing.mode === 'create'
              ? '创建知识库'
              : editing.mode === 'rename'
                ? '重命名知识库'
                : '保存预设配方'
          }
          onClose={() => setEditing(null)}
          closeLabel="关闭"
          footer={
            <>
              <button onClick={() => setEditing(null)}>取消</button>
              <button
                className="kb-primary"
                disabled={!!pending || !editing.name.trim()}
                onClick={() => void saveName()}
              >
                保存
              </button>
            </>
          }
        >
          <div className="kb-form">
            <label>
              名称
              <input
                data-drawer-autofocus
                value={editing.name}
                maxLength={256}
                onChange={(event) =>
                  setEditing((current) =>
                    current ? { ...current, name: event.target.value } : null,
                  )
                }
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && !event.nativeEvent.isComposing) void saveName()
                }}
              />
            </label>
            {editing.mode === 'create' ? (
              <label>
                建库预设
                <select
                  value={editing.preset}
                  onChange={(event) =>
                    setEditing((current) =>
                      current ? { ...current, preset: event.target.value } : null,
                    )
                  }
                >
                  {status?.presets.map((value) => (
                    <option key={value.id} value={value.id}>
                      {value.name}
                    </option>
                  ))}
                </select>
              </label>
            ) : editing.mode === 'preset' ? (
              <p className="kb-help">
                保存来源配置和规则；本地路径在新库中重新选择，正文及模型独立管理。
              </p>
            ) : null}
          </div>
        </Drawer>
      ) : null}
      {editor && status?.libraries.some((value) => value.id === editor.libraryId) ? (
        <SourceEditor
          key={`${editor.libraryId}/${editor.source?.id || 'new'}`}
          api={api}
          source={editor.source}
          libraryId={editor.libraryId}
          onClose={() => setEditor(null)}
          onSaved={() => {
            setEditor(null)
            void refresh().catch(report)
          }}
        />
      ) : null}
      {settings && status ? (
        <Settings
          api={api}
          status={status}
          onClose={() => setSettings(false)}
          onChanged={refresh}
          action={action}
          report={setError}
        />
      ) : null}
    </div>
  )
}
