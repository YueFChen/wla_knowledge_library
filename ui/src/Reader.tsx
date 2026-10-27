import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { ArrowUpRight, Download, ChevronLeft } from 'lucide-react'
import type { Api, Document } from './workspaceTypes'
import type { PageCell, PageElement, PageRun } from './types.generated'

const LinkContext = createContext<(url: string) => void>(() => {})
const safeUrl = (url: string) => /^https?:\/\//i.test(url)
function Runs({ runs }: { runs: PageRun[]; api: Api }) {
  const open = useContext(LinkContext)
  return runs.map((run, index) => {
    let content: ReactNode = run.text
    if (run.bold) content = <strong>{content}</strong>
    if (run.italic) content = <em>{content}</em>
    if (run.color && CSS.supports('color', run.color))
      content = <span style={{ color: run.color }}>{content}</span>
    return run.link && (safeUrl(run.link) || run.link.startsWith('#')) ? (
      <a
        key={index}
        href={run.link}
        onClick={(event) => {
          event.preventDefault()
          open(run.link!)
        }}
      >
        {content}
      </a>
    ) : (
      <span key={index}>{content}</span>
    )
  })
}

function OfflineImage({
  src,
  libraryId,
  api,
  width,
}: {
  src: string
  libraryId: string
  api: Api
  width?: number | null
}) {
  const root = useRef<HTMLElement>(null)
  const [image, setImage] = useState('')
  const [error, setError] = useState('')
  useEffect(() => {
    let active = true
    setImage('')
    setError('')
    const element = root.current
    if (!element || !src.startsWith('asset:')) return
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return
        observer.disconnect()
        void api
          .call<{ mime: string; base64: string }>('asset', {
            library_id: libraryId,
            key: src.slice(6),
          })
          .then((result) => {
            if (active) setImage(`data:${result.mime};base64,${result.base64}`)
          })
          .catch((reason) => {
            if (active) setError(String(reason))
          })
      },
      { rootMargin: '200px' },
    )
    observer.observe(element)
    return () => {
      active = false
      observer.disconnect()
    }
  }, [src, libraryId, api])
  return (
    <figure
      ref={root}
      className="kb-image"
      style={width && width > 0 ? { maxWidth: `min(${Math.min(width, 1200)}px, 100%)` } : undefined}
    >
      {image ? (
        <img src={image} alt="文档配图" loading="lazy" />
      ) : (
        <span>{error || (src ? '配图按需加载' : '配图未保存')}</span>
      )}
    </figure>
  )
}

function Cell({ cell, api, libraryId }: { cell: PageCell; api: Api; libraryId: string }) {
  const Tag = cell.header ? 'th' : 'td'
  return (
    <Tag colSpan={Math.min(cell.colspan, 100)} rowSpan={Math.min(cell.rowspan, 100)}>
      <Runs runs={cell.runs} api={api} />
      {cell.images?.map((src, index) => (
        <OfflineImage key={`${index}:${src}`} src={src} api={api} libraryId={libraryId} />
      ))}
    </Tag>
  )
}
function Element({
  element,
  api,
  libraryId,
}: {
  element: PageElement
  api: Api
  libraryId: string
}) {
  switch (element.type) {
    case 'heading':
      return (
        <h3
          id={`kb-anchor-${element.anchor}`}
          data-anchor={element.anchor}
          data-level={element.level}
        >
          {element.text}
        </h3>
      )
    case 'paragraph':
      return (
        <p>
          <Runs runs={element.runs} api={api} />
        </p>
      )
    case 'code':
      return (
        <pre>
          <code data-language={element.language}>{element.text}</code>
        </pre>
      )
    case 'list': {
      const Tag = element.kind === 'ordered' ? 'ol' : 'ul'
      return (
        <Tag
          style={{ marginInlineStart: `${Math.max(0, Math.min(element.indent - 1, 10)) * 1.5}em` }}
        >
          {element.items.map((runs, index) => (
            <li key={index}>
              <Runs runs={runs} api={api} />
            </li>
          ))}
        </Tag>
      )
    }
    case 'table':
      return (
        <div className="kb-table">
          <table
            style={
              element.columns.some((width) => width > 0)
                ? {
                    width: element.columns.reduce((sum, width) => sum + Math.max(0, width), 0),
                    tableLayout: 'fixed',
                  }
                : undefined
            }
          >
            {element.columns.length ? (
              <colgroup>
                {element.columns.map((width, index) => (
                  <col key={index} style={width > 0 ? { width } : undefined} />
                ))}
              </colgroup>
            ) : null}
            <tbody>
              {element.rows.map((row, index) => (
                <tr key={index}>
                  {row.map((cell, cellIndex) => (
                    <Cell key={cellIndex} cell={cell} api={api} libraryId={libraryId} />
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )
    case 'image':
      return (
        <OfflineImage src={element.src} width={element.width} api={api} libraryId={libraryId} />
      )
    case 'video_link':
      return safeUrl(element.url) ? (
        <button className="kb-link" onClick={() => void api.call('open_url', { url: element.url })}>
          <ArrowUpRight size={14} />
          查看视频来源
        </button>
      ) : null
    case 'divider':
      return <hr />
  }
}

export function Reader({
  api,
  libraryId,
  documentId,
  anchor,
  onClose,
  onError,
  onLink,
  hasResults,
}: {
  api: Api
  libraryId: string
  documentId: string
  anchor?: string
  onClose: () => void
  onError: (error: unknown) => void
  onLink: (url: string) => boolean
  hasResults: boolean
}) {
  const [document, setDocument] = useState<Document | null>(null)
  const [error, setError] = useState('')
  const [loadingMore, setLoadingMore] = useState(false)
  const [location, setLocation] = useState(anchor || '')
  const [activeAnchor, setActiveAnchor] = useState(anchor || '')
  const body = useRef<HTMLDivElement>(null)
  const requestEpoch = useRef(0)
  const shouldPosition = useRef(true)
  useEffect(() => {
    setLocation(anchor || '')
  }, [anchor])
  useEffect(() => {
    let active = true
    ++requestEpoch.current
    shouldPosition.current = true
    setDocument(null)
    setError('')
    setLoadingMore(false)
    void api
      .call<Document>('document', {
        library_id: libraryId,
        document_id: documentId,
        ...(location ? { anchor: location } : {}),
      })
      .then((value) => {
        if (active) setDocument(value)
      })
      .catch((reason) => {
        if (active) setError(String(reason))
      })
    return () => {
      active = false
      ++requestEpoch.current
    }
  }, [api, libraryId, documentId, location])
  const scrollToAnchor = (value: string) => {
    const container = body.current
    const heading = [...(container?.querySelectorAll<HTMLElement>('[data-anchor]') || [])].find(
      (element) => element.dataset.anchor === value,
    )
    if (!heading || !container) return false
    container.scrollTop +=
      heading.getBoundingClientRect().top - container.getBoundingClientRect().top - 16
    setActiveAnchor(value)
    return true
  }
  useLayoutEffect(() => {
    if (document && shouldPosition.current) {
      shouldPosition.current = false
      if (!location || !scrollToAnchor(location)) body.current?.scrollTo({ top: 0 })
    }
  }, [document, location])
  const jump = (value: string) => {
    if (!scrollToAnchor(value)) setLocation(value)
  }
  const more = async () => {
    if (!document || document.next_offset === null) return
    const epoch = requestEpoch.current
    setLoadingMore(true)
    try {
      const next = await api.call<Document>('document', {
        library_id: libraryId,
        document_id: documentId,
        offset: document.next_offset,
      })
      if (epoch === requestEpoch.current)
        setDocument((current) =>
          current
            ? {
                ...next,
                element_offset: current.element_offset,
                elements: [...current.elements, ...next.elements],
              }
            : next,
        )
    } catch (reason) {
      if (epoch === requestEpoch.current) onError(reason)
    } finally {
      if (epoch === requestEpoch.current) setLoadingMore(false)
    }
  }
  const openLink = (url: string) => {
    try {
      if (url.startsWith('#')) {
        jump(decodeURIComponent(url.slice(1)))
        return
      }
      const target = new URL(url, document?.url)
      if (!safeUrl(target.href)) return
      if (!onLink(target.href)) void api.call('open_url', { url: target.href }).catch(onError)
    } catch (reason) {
      onError(reason)
    }
  }
  return (
    <>
      <section className="kb-reader" aria-label="文档阅读">
        <div className="kb-reader-head">
          <button className="kb-link" onClick={onClose}>
            <ChevronLeft size={14} />
            {hasResults ? '返回检索结果' : '返回目录'}
          </button>
          <div className="kb-actions">
            {document && safeUrl(document.url) ? (
              <button
                className="kb-link"
                onClick={() => void api.call('open_url', { url: document.url }).catch(onError)}
              >
                <ArrowUpRight size={14} />
                访问来源
              </button>
            ) : null}
            <button
              className="kb-icon"
              aria-label="导出原文"
              onClick={() =>
                void api
                  .call('document_export', { library_id: libraryId, document_id: documentId })
                  .catch(onError)
              }
            >
              <Download size={14} />
            </button>
          </div>
        </div>
        <div
          className="kb-reader-scroll"
          ref={body}
          onScroll={() => {
            const container = body.current
            if (!container) return
            const limit = container.getBoundingClientRect().top + 100
            const headings = [...container.querySelectorAll<HTMLElement>('[data-anchor]')]
            const current =
              headings.filter((heading) => heading.getBoundingClientRect().top <= limit).at(-1) ||
              headings[0]
            if (current) setActiveAnchor(current.dataset.anchor || '')
          }}
        >
          {error ? (
            <p className="kb-error" role="alert">
              {error}
            </p>
          ) : document ? (
            <>
              <header>
                <h2>{document.title}</h2>
                <p className="kb-help">{document.metadata.breadcrumb.join(' / ')}</p>
                {document.element_offset > 0 ? (
                  <button className="kb-link" onClick={() => setLocation('')}>
                    从头阅读
                  </button>
                ) : null}
              </header>
              <LinkContext.Provider value={openLink}>
                <article>
                  {document.elements.map((element, index) => (
                    <Element
                      key={`${document.id}:${document.element_offset + index}`}
                      element={element}
                      api={api}
                      libraryId={libraryId}
                    />
                  ))}
                </article>
              </LinkContext.Provider>
              {document.next_offset !== null ? (
                <button disabled={loadingMore} onClick={() => void more()}>
                  {loadingMore ? '读取中…' : '继续阅读'}
                </button>
              ) : null}
            </>
          ) : (
            <p className="kb-help" role="status">
              正在读取资料…
            </p>
          )}
        </div>
      </section>
      <aside className="kb-outline" aria-label="文章大纲">
        <h3>大纲</h3>
        <nav>
          {document?.outline.map((heading) => (
            <button
              key={`${heading.index}:${heading.anchor}`}
              className={activeAnchor === heading.anchor ? 'active' : ''}
              style={{ paddingInlineStart: `${10 + Math.max(0, heading.level - 1) * 10}px` }}
              aria-current={activeAnchor === heading.anchor ? 'location' : undefined}
              title={heading.text}
              onClick={() => jump(heading.anchor)}
            >
              {heading.text}
            </button>
          ))}
        </nav>
      </aside>
    </>
  )
}
