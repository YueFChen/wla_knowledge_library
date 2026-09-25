import type { MessageKey } from './i18n'

/**
 * 命中来源 → 文案 key。
 *
 * 取值与 MCP 工具返回的 `kind` 一一对应（候选、词法或本地向量召回），
 * 界面与外部客户端看到的是同一套词。
 */
export const KIND_LABEL: Record<string, MessageKey> = {
  candidate_node: 'hit.candidate_node',
  candidate_doc: 'hit.candidate_doc',
  sparse: 'hit.sparse',
  vector: 'hit.vector',
}

/** 归属端 → 文案 key；非节点块没有这一项。 */
export const SIDE_LABEL: Record<string, MessageKey> = {
  server: 'hit.server',
  client: 'hit.client',
  both: 'hit.both',
}

/** 有映射就取文案 key，没有则返回 undefined（调用方按原样处理）。 */
export const labelKey = (map: Record<string, MessageKey>, value: string | null) =>
  value === null ? undefined : map[value]
