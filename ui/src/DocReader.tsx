import type { MirroredPage } from './types.generated'
import { ImageOff } from 'lucide-react'

import { t } from './i18n'
import { PageRenderer } from './PageRenderer'

interface Props {
  pathId: string
  /** 从目录树 / 检索结果带过来的标题：镜像读回来之前也有标题可显示。 */
  title: string | null
  /** 已镜像的页面元素；只有它有内容可渲染。 */
  page: MirroredPage | null
  /** 正在读镜像。 */
  loading: boolean
  /** 正在镜像本篇。 */
  mirroring: boolean
  onMirror: (pathId: string) => void
  onBack: () => void
  onOpenSource: (pathId: string) => void
  onOpenDoc: (pathId: string) => void
}

/**
 * 中间内容区：一篇文档。
 *
 * 这一版只按镜像内容展示——静态语料里的纯文本块（表格被压平、图片只剩"如图"）
 * 会让人以为官方页面就长这样，索性撤掉那个降级档位：没镜像就明说，并就地给「镜像本篇」。
 *
 * 自己不带边框：外层 `.ka-main` 已经是滚动面板，正文只管往里铺。
 */
export function DocReader({
  pathId,
  title,
  page,
  loading,
  mirroring,
  onMirror,
  onBack,
  onOpenSource,
  onOpenDoc,
}: Props) {
  return (
    <section className="ka-doc">
      <header className="ka-doc-head">
        <button className="ka-ghost" onClick={onBack}>
          ← {t('doc.back')}
        </button>
        <button className="ka-ghost" onClick={() => onOpenSource(pathId)}>
          {t('common.openSource')}
        </button>
      </header>

      <h2 className="ka-doc-heading">{page?.title ?? title ?? pathId}</h2>

      {page ? (
        <>
          <p className="ka-meta">
            {t('mirror.mirroredAt', { time: new Date(page.mirrored_at * 1000).toLocaleString('zh-CN') })}
          </p>
          <PageRenderer page={page} onOpenDoc={onOpenDoc} />
        </>
      ) : loading ? (
        <p className="ka-placeholder" role="status">
          {t('common.loading')}
        </p>
      ) : (
        <>
          <p className="ka-hint">{t('doc.notMirrored')}</p>
          <button className="ka-primary" disabled={mirroring} onClick={() => onMirror(pathId)}>
            <ImageOff aria-hidden />
            {mirroring ? t('mirror.mirroring') : t('mirror.mirrorThis')}
          </button>
        </>
      )}
    </section>
  )
}
