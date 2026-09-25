import { useEffect, useMemo, useState } from 'react'
import type { KnowledgeDocBrief } from './types.generated'
import { ChevronDown, ChevronRight, FileText, FolderTree } from 'lucide-react'

import type { KnowledgeApi } from './api'
import { errorMessage } from './display'
import { t } from './i18n'

/** 目录树的一节：目录页是分组（`children` 非空），叶子文档才可点开。 */
interface TreeNode {
  /** 从分区起算的整条标题路径，同一棵树里唯一（展开态用它做 key）。 */
  key: string
  label: string
  /** 挂在这一节上的文档；只有分区本身或中间目录没有正文时为 null。 */
  doc: KnowledgeDocBrief | null
  children: TreeNode[]
}

interface Props {
  api: KnowledgeApi
  /** 变化即重取清单（抓取/镜像后目录与标题可能都变了）。 */
  reloadToken: number
  /** 当前打开的篇目：树上高亮。 */
  activePathId: string | null
  onOpen: (pathId: string, title: string) => void
}

/**
 * 左侧目录树：`section` 顶层分区 → 面包屑层级 → 文档。
 *
 * 用 `docs([])` 一次拿全量清单，过滤放在前端做：清单只有两百来篇、一次就取完，
 * 每敲一个字再走一趟 IPC 没有收益。目录节点也有自己的正文，因此分组行把「展开」和
 * 「打开正文」拆成两个按钮，不能因为它有 children 就吞掉自己的文档入口。
 */
export function DocTree({ api, reloadToken, activePathId, onOpen }: Props) {
  const [docs, setDocs] = useState<KnowledgeDocBrief[] | null>(null)
  const [filter, setFilter] = useState('')
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set())
  const [error, setError] = useState('')

  useEffect(() => {
    let live = true
    void api
      .docs([])
      .then((groups) => {
        if (live) setDocs(groups.flatMap((group) => group.docs))
      })
      .catch((cause) => {
        if (live) setError(errorMessage(cause, t('common.failed')))
      })
    return () => {
      live = false
    }
  }, [api, reloadToken])

  const needle = filter.trim().toLowerCase()
  const visible = useMemo(() => {
    if (!docs) return []
    if (!needle) return docs
    return docs.filter((doc) => `${doc.title} ${doc.breadcrumb.join(' ')}`.toLowerCase().includes(needle))
  }, [docs, needle])
  const tree = useMemo(() => buildTree(visible), [visible])
  // 过滤时一律展开：否则命中会被先前的折叠状态藏起来，看起来像"没结果"。
  const filtering = needle.length > 0

  const toggle = (key: string) =>
    setCollapsed((previous) => {
      const next = new Set(previous)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })

  return (
    <aside className="ka-tree">
      <div className="ka-tree-head">
        <h2>
          <FolderTree aria-hidden />
          {t('tree.title')}
        </h2>
        <span className="ka-meta">{t('tree.total', { count: visible.length })}</span>
      </div>

      <input
        className="ka-input"
        value={filter}
        placeholder={t('tree.filter')}
        onChange={(event) => setFilter(event.target.value)}
      />

      {error && (
        <div className="ka-error" role="alert">
          {error}
        </div>
      )}

      {!docs ? (
        <p className="ka-placeholder" role="status">
          {t('common.loading')}
        </p>
      ) : visible.length === 0 ? (
        <p className="ka-placeholder">{t('tree.empty')}</p>
      ) : (
        <ul className="ka-tree-body">
          {tree.map((node) => (
            <Branch
              key={node.key}
              node={node}
              depth={0}
              collapsed={collapsed}
              filtering={filtering}
              activePathId={activePathId}
              onToggle={toggle}
              onOpen={onOpen}
            />
          ))}
        </ul>
      )}
    </aside>
  )
}

interface BranchProps {
  node: TreeNode
  depth: number
  collapsed: Set<string>
  /** 正在过滤：忽略折叠状态，命中一律露出来。 */
  filtering: boolean
  activePathId: string | null
  onToggle: (key: string) => void
  onOpen: (pathId: string, title: string) => void
}

function Branch({ node, depth, collapsed, filtering, activePathId, onToggle, onOpen }: BranchProps) {
  const doc = node.doc
  const expandable = node.children.length > 0
  const expanded = filtering || !collapsed.has(node.key)
  // 层级缩进按深度算：默认展开时一眼能看出分区的从属关系。
  const indent = { paddingLeft: `${0.55 + depth * 0.8}rem` }

  return (
    <li>
      {expandable ? (
        <div className="ka-tree-group-row" style={indent}>
          <button className="ka-tree-toggle" aria-expanded={expanded} onClick={() => onToggle(node.key)}>
            {expanded ? <ChevronDown aria-hidden /> : <ChevronRight aria-hidden />}
          </button>
          <button
            className={doc?.path_id === activePathId ? 'ka-tree-group ka-tree-group--active' : 'ka-tree-group'}
            onClick={() => (doc ? onOpen(doc.path_id, doc.title) : onToggle(node.key))}
          >
            {doc && <FileText aria-hidden />}
            <span className="ka-tree-label">{node.label}</span>
            <span className="ka-tree-count">{node.children.length}</span>
          </button>
        </div>
      ) : doc ? (
        <button
          className={doc.path_id === activePathId ? 'ka-tree-leaf ka-tree-leaf--active' : 'ka-tree-leaf'}
          style={indent}
          onClick={() => onOpen(doc.path_id, doc.title)}
        >
          <FileText aria-hidden />
          <span className="ka-tree-label">{doc.title}</span>
        </button>
      ) : null}

      {expandable && expanded && (
        <ul className="ka-tree-branch">
          {node.children.map((child) => (
            <Branch
              key={child.key}
              node={child}
              depth={depth + 1}
              collapsed={collapsed}
              filtering={filtering}
              activePathId={activePathId}
              onToggle={onToggle}
              onOpen={onOpen}
            />
          ))}
        </ul>
      )}
    </li>
  )
}

/**
 * 清单 → 树。
 *
 * 每篇的 `breadcrumb` 自带顶层分区（第一段就是 `section`、最后一段是标题本身），
 * 所以按面包屑逐段下钻即可，缺的中间节点由子节点补出来，不依赖清单的顺序。
 */
function buildTree(docs: KnowledgeDocBrief[]): TreeNode[] {
  const roots: TreeNode[] = []
  const index = new Map<string, TreeNode>()

  for (const doc of docs) {
    const rest = doc.breadcrumb[0] === doc.section ? doc.breadcrumb.slice(1) : doc.breadcrumb
    const path = [doc.section, ...(rest.length > 0 ? rest : [doc.title])]
    let level = roots
    let key = ''
    path.forEach((label, position) => {
      key = key ? `${key}/${label}` : label
      let node = index.get(key)
      if (!node) {
        node = { key, label, doc: null, children: [] }
        index.set(key, node)
        level.push(node)
      }
      if (position === path.length - 1) node.doc = doc
      level = node.children
    })
  }

  return roots
}
