import { useState } from 'react'
import { Drawer } from '@wonderland/ui'
import { FolderOpen } from 'lucide-react'
import type { Api, Source } from './workspaceTypes'

export function SourceEditor({
  api,
  source,
  libraryId,
  onClose,
  onSaved,
}: {
  api: Api
  source?: Source
  libraryId: string
  onClose: () => void
  onSaved: () => void
}) {
  const [draft, setDraft] = useState<Source>(
    () =>
      source ?? {
        id: '',
        name: '',
        kind: 'directory',
        paths: [],
        url: '',
        path_prefix: '',
        depth: 0,
        max_pages: 100,
        dynamic: false,
      },
  )
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const patch = (value: Partial<Source>) => setDraft((current) => ({ ...current, ...value }))
  const pick = async () => {
    setBusy(true)
    setError('')
    try {
      const result = await api.call<{ cancelled: boolean; paths: string[] }>('pick_paths', {
        kind: draft.kind === 'directory' ? 'directory' : 'files',
        extensions: draft.kind === 'directory' ? [] : ['md', 'markdown', 'txt', 'html', 'htm'],
      })
      if (!result.cancelled)
        patch({
          paths: result.paths,
          name: draft.name || result.paths[0]?.split(/[\\/]/).filter(Boolean).pop() || '本地资料',
        })
    } catch (reason) {
      setError(String(reason))
    } finally {
      setBusy(false)
    }
  }
  const save = async () => {
    setBusy(true)
    setError('')
    try {
      if (!draft.name.trim()) throw new Error('请输入资料源名称')
      if ((draft.kind === 'directory' || draft.kind === 'files') && !draft.paths.length)
        throw new Error('请选择资料路径')
      await api.call('source_save', { library_id: libraryId, source: draft })
      onSaved()
    } catch (reason) {
      setError(String(reason))
    } finally {
      setBusy(false)
    }
  }
  return (
    <Drawer
      open
      title={source ? '编辑资料源' : '添加资料源'}
      onClose={onClose}
      closeLabel="关闭"
      footer={
        <>
          <button onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="kb-primary" disabled={busy} onClick={() => void save()}>
            {busy ? '正在处理…' : '保存资料源'}
          </button>
        </>
      }
    >
      <div className="kb-form">
        <label>
          来源类型
          <select
            value={draft.kind}
            onChange={(event) =>
              patch({
                kind: event.target.value as Source['kind'],
                paths: [],
                url: '',
                name: '',
                dynamic: event.target.value === 'wonderland',
                max_pages: event.target.value === 'wonderland' ? 10000 : 100,
              })
            }
          >
            <option value="directory">本地目录</option>
            <option value="files">本地文件</option>
            <option value="website">网页或站点</option>
            <option value="wonderland">千星奇域官方资料</option>
          </select>
        </label>
        <label>
          名称
          <input
            value={draft.name}
            maxLength={256}
            onChange={(event) => patch({ name: event.target.value })}
            placeholder="例如：开发笔记"
          />
        </label>
        {draft.kind === 'directory' || draft.kind === 'files' ? (
          <div>
            <button disabled={busy} onClick={() => void pick()}>
              <FolderOpen size={16} />
              {draft.kind === 'directory' ? '选择目录' : '选择文件'}
            </button>
            <p className="kb-help">
              支持 Markdown、TXT、HTML。更新时扫描原始来源，阅读使用库内快照。
            </p>
            <div className="kb-paths">
              {draft.paths.map((path) => (
                <div key={path} title={path}>
                  {path}
                </div>
              ))}
            </div>
          </div>
        ) : draft.kind === 'website' ? (
          <>
            <label>
              入口网址
              <input
                type="url"
                value={draft.url}
                onChange={(event) => patch({ url: event.target.value })}
                placeholder="https://example.com/docs/"
              />
            </label>
            <label>
              站内路径范围
              <input
                value={draft.path_prefix}
                onChange={(event) => patch({ path_prefix: event.target.value })}
                placeholder="/docs（留空为整个站点）"
              />
            </label>
            <div className="kb-form-row">
              <label>
                遍历深度
                <input
                  type="number"
                  min={0}
                  max={10}
                  value={draft.depth}
                  onChange={(event) => patch({ depth: Number(event.target.value) })}
                />
              </label>
              <label>
                最多页面
                <input
                  type="number"
                  min={1}
                  max={10000}
                  value={draft.max_pages}
                  onChange={(event) => patch({ max_pages: Number(event.target.value) })}
                />
              </label>
            </div>
            <label className="kb-check">
              <input
                type="checkbox"
                checked={draft.dynamic}
                onChange={(event) => patch({ dynamic: event.target.checked })}
              />
              使用系统 Edge 渲染动态页面
            </label>
            <p className="kb-help">
              深度 0 只获取入口页。采集无需登录的正文和配图，附件按需另行导入。
            </p>
          </>
        ) : (
          <>
            <p className="kb-help">按官方目录构建千星资料，保留节点、端侧及 Lua API 检索扩展。</p>
            <label>
              最多正文页数
              <input
                type="number"
                min={1}
                max={10000}
                value={draft.max_pages}
                onChange={(event) => patch({ max_pages: Number(event.target.value) })}
              />
            </label>
          </>
        )}
        {error ? (
          <p className="kb-error" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    </Drawer>
  )
}
