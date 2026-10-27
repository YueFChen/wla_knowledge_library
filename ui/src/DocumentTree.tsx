import { useMemo, useState } from 'react'
import { FileText, Search } from 'lucide-react'
import type { Brief, Source } from './workspaceTypes'

interface Node {
  name: string
  children: Map<string, Node>
  documents: Brief[]
}
export function DocumentTree({
  documents,
  sources,
  active,
  open,
}: {
  documents: Brief[]
  sources: Source[]
  active?: string
  open: (id: string) => void
}) {
  const [filter, setFilter] = useState('')
  const tree = useMemo(() => {
    const root: Node = { name: '', children: new Map(), documents: [] }
    for (const document of documents) {
      if (
        filter &&
        !`${document.title} ${document.breadcrumb.join(' ')}`
          .toLowerCase()
          .includes(filter.toLowerCase())
      )
        continue
      const path =
        document.breadcrumb.length > 1
          ? document.breadcrumb.slice(0, -1)
          : [
              document.section ||
                sources.find((source) => source.id === document.source_id)?.name ||
                '资料',
            ]
      let node = root
      for (const name of path) {
        if (!node.children.has(name))
          node.children.set(name, { name, children: new Map(), documents: [] })
        node = node.children.get(name)!
      }
      node.documents.push(document)
    }
    return root
  }, [documents, sources, filter])
  const render = (node: Node, prefix: string): React.ReactNode => (
    <>
      {[...node.children].map(([name, child]) => (
        <details key={name} open={filter ? true : undefined} className="kb-tree-group">
          <summary>{name}</summary>
          {render(child, `${prefix}/${name}`)}
        </details>
      ))}
      {node.documents.map((document) => (
        <button
          key={`${prefix}/${document.id}`}
          className={`kb-tree-leaf${active === document.id ? ' active' : ''}`}
          aria-current={active === document.id ? 'page' : undefined}
          title={document.title}
          onClick={() => open(document.id)}
        >
          <FileText size={14} />
          <span>{document.title}</span>
        </button>
      ))}
    </>
  )
  return (
    <aside className="kb-tree" aria-label="资料目录">
      <label className="kb-tree-filter">
        <Search size={14} />
        <input
          aria-label="筛选目录"
          placeholder="筛选目录"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
      </label>
      <div className="kb-tree-scroll">
        {render(tree, '')}
        {!documents.length ? <p className="kb-help">暂无资料</p> : null}
      </div>
    </aside>
  )
}
