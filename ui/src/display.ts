// 本模块只放**纯取值 / 格式化**逻辑，不引入运行时依赖（含文案模块）：
// 这样单测能直接用 `node --experimental-strip-types` 跑，无需打包步骤。
// 文案与类名拼接由调用方按状态映射，见各面板组件。

import type { MirroredPage, PageElement } from './types.generated'

/** 镜像页面里的标题元素（右侧文章大纲用）。 */
export type PageHeading = Extract<PageElement, { type: 'heading' }>

/** Unix 秒 → 本地化字符串；没抓过或时间戳异常时显示占位符。 */
export const formatTime = (seconds: number | null) =>
  seconds !== null && seconds > 0 ? new Date(seconds * 1000).toLocaleString('zh-CN') : '—'

/** IPC 错误载荷优先取 message，其余落到调用方给的兜底文案。 */
export const errorMessage = (error: unknown, fallback: string) =>
  error && typeof error === 'object' && 'message' in error ? String(error.message) : fallback

/** 字节数 → 可读单位（抓取报告用）。 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`
}

/** 缓存天数 → 取整的天数（界面只说"几天前"，不显示小数）。 */
export const wholeDays = (days: number | null) => (days === null ? null : Math.max(0, Math.round(days)))

/**
 * 一趟输入 → 去重后的条目。
 *
 * 检索栏是单行输入，多个问题用逗号隔开（换行一并认，粘贴进来的多行文本不用先手工合并）；
 * 空白与重复一律丢弃：MCP 侧同样把多个查询合并成一次调用，界面沿用同一套输入习惯。
 */
export function splitLines(text: string): string[] {
  const items = text
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean)
  return [...new Set(items)]
}

/** 面包屑 → 一行可读的路径（消歧用，最长的那个在最后）。 */
export const breadcrumbText = (breadcrumb: string[]) => breadcrumb.join(' / ')

/**
 * 官方详情页链接 → `path_id`。
 *
 * 正文里的站内链接形如 `https://act.mihoyo.com/ys/ugc/tutorial/detail/<path_id>`；
 * 只有这种链接才能在应用内打开（宿主只接受 `path_id`，不接受任意地址）。
 */
export function docPathId(link: string): string | null {
  const match = /\/detail\/([A-Za-z0-9_-]+)\/?$/.exec(link.trim())
  return match ? match[1] : null
}

/** 元素锚点 → DOM id（加前缀避免与页面自身的 id 冲突）。 */
export const anchorId = (anchor: string) => `ka-anchor-${anchor}`

/** 在已渲染的正文里寻找与检索块最接近的实际元素，并滚到该段、列表项或表格行。 */
export function scrollToText(root: HTMLElement, rawText: string): boolean {
  const compact = (value: string) => value.toLocaleLowerCase().replace(/[\s\p{P}\p{S}]/gu, '')
  const target = compact(rawText)
  if (target.length < 3) return false

  const candidates = Array.from(root.querySelectorAll<HTMLElement>('h3.ka-h, p.ka-p, li, tr, table'))
    .map((element) => ({ element, text: compact(element.textContent ?? '') }))
    .filter(({ text }) => text.length >= 3 && (text === target || text.includes(target) || target.includes(text)))
    .sort((left, right) => {
      const leftExact = left.text === target ? 0 : 1
      const rightExact = right.text === target ? 0 : 1
      return leftExact - rightExact || Math.abs(left.text.length - target.length) - Math.abs(right.text.length - target.length)
    })

  const match = candidates[0]?.element
  if (!match) return false
  match.scrollIntoView({ behavior: 'smooth', block: 'center' })
  match.classList.add('ka-search-target')
  window.setTimeout(() => match.classList.remove('ka-search-target'), 1800)
  return true
}

/** 正文里的标题，按出现顺序（正文大纲按这个顺序列，也是滚动高亮的判定顺序）。 */
export const pageHeadings = (page: MirroredPage): PageHeading[] =>
  page.elements.flatMap((element) => (element.type === 'heading' ? [element] : []))

/**
 * 图片地址按官方给的显示宽度收窄，并给一个上限。
 *
 * 上限取「官方宽度」与「容器宽度」的较小值：只写官方宽度会盖掉样式表里的
 * `max-width: 100%`，宽图（官方不少截图是 1200px 以上）会把正文撑出横向滚动。
 */
export const imageStyle = (width: number | null | undefined) =>
  width && width > 0 ? { maxWidth: `min(${Math.min(width, 1200)}px, 100%)` } : undefined
