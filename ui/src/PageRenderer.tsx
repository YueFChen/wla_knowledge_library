import { Fragment, useState } from 'react'
import type { MirroredPage, PageCell, PageElement, PageRun } from './types.generated'

import { anchorId, docPathId, imageStyle } from './display'
import { t } from './i18n'

interface Props {
  page: MirroredPage
  /** 正文里的站内链接按 `path_id` 在应用内打开。 */
  onOpenDoc: (pathId: string) => void
}

/**
 * 官方详情页的还原。
 *
 * 只还原官方排版里**有内容含义**的那几层：标题层级（含目录）、段落内联样式、表格、
 * 列表、图片、分隔线。官方页面里那些布局用的 `div` / `span` / 图标不还原——它们是版式实现，
 * 不是内容（调研 §13.5 的取舍）。
 *
 * 标题导航不放这里：它在整页排版的右侧栏（见 `Outline`），跟着正文滚动高亮；
 * 正文只管把官方内容原样铺开。
 */
export function PageRenderer({ page, onOpenDoc }: Props) {
  return (
    <article className="ka-elements">
      {page.elements.map((element, index) => (
        <Block key={index} element={element} onOpenDoc={onOpenDoc} />
      ))}
    </article>
  )
}

function Block({ element, onOpenDoc }: { element: PageElement; onOpenDoc: (pathId: string) => void }) {
  switch (element.type) {
    case 'heading':
      // 页面里已经有标题层级（h1 页头 + h2 面板标题），正文从 h3 起，语义上仍保持官方层级。
      return (
        <h3 id={anchorId(element.anchor)} className={`ka-h ka-h--${element.level}`}>
          {element.text}
        </h3>
      )
    case 'paragraph':
      return (
        <p className="ka-p">
          <Runs runs={element.runs} onOpenDoc={onOpenDoc} />
        </p>
      )
    case 'list': {
      const Tag = element.kind === 'ordered' ? 'ol' : 'ul'
      return (
        <Tag className="ka-list" style={{ marginLeft: `${(element.indent - 1) * 1.2}rem` }}>
          {element.items.map((item, index) => (
            <li key={index}>
              <Runs runs={item} onOpenDoc={onOpenDoc} />
            </li>
          ))}
        </Tag>
      )
    }
    case 'table':
      return (
        <div className="ka-table-wrap">
          <table className="ka-table">
            {element.columns.some((width) => width > 0) && (
              <colgroup>
                {element.columns.map((width, index) => (
                  <col key={index} style={width > 0 ? { width: `${width}px` } : undefined} />
                ))}
              </colgroup>
            )}
            <tbody>
              {element.rows.map((row, rowIndex) => (
                <tr key={rowIndex}>
                  {row.map((cell, cellIndex) => (
                    <Cell key={cellIndex} cell={cell} onOpenDoc={onOpenDoc} />
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )
    case 'image':
      return <Figure src={element.src} width={element.width} />
    case 'video_link':
      return (
        <p className="ka-p">
          <a className="ka-external-link" href={element.url} rel="noreferrer">
            {t('page.openVideo')}
          </a>
        </p>
      )
    case 'divider':
      return <hr className="ka-hr" />
    default:
      return null
  }
}

function Cell({ cell, onOpenDoc }: { cell: PageCell; onOpenDoc: (pathId: string) => void }) {
  const Tag = cell.header ? 'th' : 'td'
  return (
    <Tag className="ka-cell" colSpan={cell.colspan} rowSpan={cell.rowspan}>
      {cell.runs.length > 0 && (
        <span className="ka-cell-text">
          <Runs runs={cell.runs} onOpenDoc={onOpenDoc} />
        </span>
      )}
      {(cell.images ?? []).map((src) => (
        <Figure key={src} src={src} />
      ))}
    </Tag>
  )
}

/** 内联文本：加粗 / 斜体 / 官方着色 / 换行 / 站内跳转。 */
function Runs({ runs, onOpenDoc }: { runs: PageRun[]; onOpenDoc?: (pathId: string) => void }) {
  return (
    <>
      {runs.map((run, index) => {
        if (run.text === '\n') return <br key={index} />
        const style = run.color ? { color: run.color } : undefined
        const target = run.link ? docPathId(run.link) : null
        if (target && onOpenDoc) {
          return (
            <button
              key={index}
              className="ka-inline-link"
              style={style}
              title={run.link ?? undefined}
              onClick={() => onOpenDoc(target)}
            >
              {run.text}
            </button>
          )
        }
        if (run.link) {
          return (
            <a key={index} className="ka-external-link" href={run.link} rel="noreferrer">
              {run.text}
            </a>
          )
        }
        if (run.link) {
          return (
            <a key={index} className="ka-external-link" href={run.link} rel="noreferrer">
              {run.text}
            </a>
          )
        }
        const marks = [run.bold ? 'ka-b' : '', run.italic ? 'ka-i' : ''].filter(Boolean).join(' ')
        if (!style && !marks) return <Fragment key={index}>{run.text}</Fragment>
        return (
          <span key={index} className={marks || undefined} style={style}>
            {run.text}
          </span>
        )
      })}
    </>
  )
}

/** 正文图：直接引官方 CDN（只记地址不下载），加载失败给一条可读提示而不是破图。 */
function Figure({ src, width }: { src: string; width?: number | null }) {
  const [failed, setFailed] = useState(false)
  if (failed) return <span className="ka-warn">{t('page.imageFailed')}</span>
  return (
    <img
      className="ka-image"
      src={src}
      style={imageStyle(width)}
      loading="lazy"
      alt=""
      onError={() => setFailed(true)}
    />
  )
}
