import type { KnowledgeHit, KnowledgeSearchGroup } from './types.generated'

import { breadcrumbText } from './display'
import { t } from './i18n'
import { KIND_LABEL, SIDE_LABEL, labelKey } from './labels'

interface Props {
  groups: KnowledgeSearchGroup[]
  canGoBack: boolean
  onBack: () => void
  /** 点一条：在右侧打开该篇并跳到命中锚点。 */
  onOpen: (pathId: string, title: string, anchor: string, text: string) => void
}

/**
 * 检索结果：整块占右侧内容区，一行一条。
 *
 * 结果与正文共用一个位置（而不是弹层或独立面板）——检索之后十有八九要读命中那一段，
 * 让它直接顶掉右侧，点一下就到位，不必先开一个浮层再开正文。
 */
export function SearchResults({ groups, canGoBack, onBack, onOpen }: Props) {
  return (
    <>
      {canGoBack && (
        <button className="ka-ghost ka-results-back" onClick={onBack}>
          ← {t('doc.back')}
        </button>
      )}
      {groups.map((group) => (
        <section className="ka-group" key={group.query}>
          <h3 className="ka-group-title">{group.query}</h3>
          {group.hits.length === 0 ? (
            <p className="ka-placeholder">{t('search.empty')}</p>
          ) : (
            group.hits.map((hit) => (
              <ResultRow key={`${hit.path_id}/${hit.anchor}/${hit.text.slice(0, 24)}`} hit={hit} onOpen={onOpen} />
            ))
          )}
        </section>
      ))}
    </>
  )
}

/** 一条命中：标题 + 面包屑 + 命中来源 / 端点标记 + 片段；整行就是「打开」这一个动作。 */
function ResultRow({ hit, onOpen }: { hit: KnowledgeHit; onOpen: Props['onOpen'] }) {
  const kind = labelKey(KIND_LABEL, hit.kind)
  const side = labelKey(SIDE_LABEL, hit.side)
  return (
    <button className="ka-result" onClick={() => onOpen(hit.path_id, hit.doc_title, hit.anchor, hit.text)}>
      <span className="ka-result-head">
        <span className="ka-result-title">{hit.doc_title}</span>
        {kind && <span className="ka-tag">{t(kind)}</span>}
        {side && <span className="ka-tag ka-tag--side">{t(side)}</span>}
        {hit.node_graph && <span className="ka-tag ka-tag--graph">{hit.node_graph}</span>}
      </span>
      {hit.breadcrumb.length > 0 && <span className="ka-crumb">{breadcrumbText(hit.breadcrumb)}</span>}
      <span className="ka-result-text">{hit.text}</span>
    </button>
  )
}
