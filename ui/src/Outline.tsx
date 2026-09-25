import { useEffect, useRef, useState, type RefObject } from 'react'

import { anchorId, type PageHeading } from './display'
import { t } from './i18n'

interface Props {
  headings: PageHeading[]
  /** 正文所在的滚动容器：大纲按它的滚动位置定位当前小节。 */
  scrollRef: RefObject<HTMLElement | null>
}

/**
 * 右侧文章大纲：按标题层级列出，随正文滚动高亮当前小节。
 *
 * 高亮位置用「容器滚动量」算而不是 IntersectionObserver：正文里的图是懒加载的，
 * 图片落地会把后面的标题整体推移，每次滚动都读一遍最新位置就不会错位。
 */
export function Outline({ headings, scrollRef }: Props) {
  const [active, setActive] = useState<string | null>(null)
  const listRef = useRef<HTMLUListElement | null>(null)
  const activeRef = useRef<HTMLButtonElement | null>(null)

  useEffect(() => {
    const root = scrollRef.current
    if (!root || headings.length === 0) return
    const update = () => {
      // 判定线放在容器上沿往下三分之一处：标题越过它才算「正在读这一节」。
      const line = root.getBoundingClientRect().top + root.clientHeight / 3
      let current = headings[0].anchor
      for (const heading of headings) {
        const element = document.getElementById(anchorId(heading.anchor))
        if (element && element.getBoundingClientRect().top <= line) current = heading.anchor
      }
      setActive(current)
    }
    update()
    root.addEventListener('scroll', update, { passive: true })
    window.addEventListener('resize', update)
    return () => {
      root.removeEventListener('scroll', update)
      window.removeEventListener('resize', update)
    }
  }, [headings, scrollRef])

  // 当前小节滚出大纲可视区时把大纲也带着走：否则读到后半篇时大纲还停在前几节。
  useEffect(() => {
    const list = listRef.current
    const item = activeRef.current
    if (!list || !item) return
    const listBox = list.getBoundingClientRect()
    const box = item.getBoundingClientRect()
    if (box.top < listBox.top) list.scrollTop -= listBox.top - box.top + 8
    else if (box.bottom > listBox.bottom) list.scrollTop += box.bottom - listBox.bottom + 8
  }, [active])

  // 官方页面的标题层级不保证从 1 起（有的篇目直接从 h2 开始），缩进按最小层级算差。
  const base = Math.min(...headings.map((heading) => heading.level))

  return (
    <aside className="ka-outline" aria-label={t('page.toc')}>
      <h2 className="ka-outline-title">{t('page.toc')}</h2>
      <ul className="ka-outline-list" ref={listRef}>
        {headings.map((heading) => {
          const current = heading.anchor === active
          return (
            <li key={heading.anchor}>
              <button
                ref={current ? activeRef : undefined}
                className={current ? 'ka-outline-item ka-outline-item--active' : 'ka-outline-item'}
                style={{ paddingLeft: `${0.5 + (heading.level - base) * 0.7}rem` }}
                onClick={() =>
                  document
                    .getElementById(anchorId(heading.anchor))
                    ?.scrollIntoView({ behavior: 'smooth', block: 'start' })
                }
              >
                {heading.text}
              </button>
            </li>
          )
        })}
      </ul>
    </aside>
  )
}
