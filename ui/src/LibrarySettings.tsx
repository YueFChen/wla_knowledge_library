import { Drawer } from '@wonderland/ui'
import type { Library, Status } from './workspaceTypes'
export function LibrarySettings({
  library,
  status,
  close,
  global,
  action,
  busy,
}: {
  library: Library
  status: Status
  close: () => void
  global: () => void
  action: (method: string, params?: Record<string, unknown>) => Promise<unknown>
  busy: boolean
}) {
  const bound = status.models.some((model) => model.id === library.model)
  return (
    <Drawer open title="当前库设置" onClose={close} closeLabel="关闭">
      <div className="kb-form">
        <h3>{library.name}</h3>
        <label>
          检索模型
          <select
            disabled={busy}
            value={bound ? library.model! : ''}
            onChange={(event) =>
              void action('library_update', {
                library_id: library.id,
                model: event.target.value || null,
              })
            }
          >
            <option value="">仅关键词</option>
            {status.models.map((model) => (
              <option key={model.id} value={model.id}>
                {model.name} ·{' '}
                {status.services.find((service) => service.id === model.service_id)?.name}
              </option>
            ))}
          </select>
        </label>
        {library.model && !bound ? (
          <p className="kb-warning">原模型需要重新绑定本机服务。</p>
        ) : library.model_requirement && !library.model ? (
          <p className="kb-help">
            原模型：{library.model_requirement.model} · {library.model_requirement.dimensions}{' '}
            维。请选择本机模型后重建索引。
          </p>
        ) : null}
        <div className="kb-actions">
          <button className="kb-link" onClick={global}>
            管理模型服务
          </button>
        </div>
        <section className="kb-form">
          <h3>语义索引</h3>
          <p className="kb-help">
            {bound ? (library.vector_revision ? '已构建' : '待构建') : '未启用'}
          </p>
          <button
            disabled={busy || !bound || !library.snapshot}
            onClick={() => void action('build', { library_id: library.id, semantic: true })}
          >
            构建语义索引
          </button>
        </section>
      </div>
    </Drawer>
  )
}
