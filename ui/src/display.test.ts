import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  anchorId,
  breadcrumbText,
  docPathId,
  errorMessage,
  formatBytes,
  formatTime,
  imageStyle,
  splitLines,
  wholeDays,
} from './display.ts'

test('formatTime：没抓过与异常时间戳都显示占位符', () => {
  assert.equal(formatTime(null), '—')
  assert.equal(formatTime(0), '—')
  assert.ok(formatTime(1_700_000_000).length > 0)
})

test('errorMessage：优先取 IPC 载荷里的 message', () => {
  assert.equal(errorMessage({ code: 'local_data', message: '读缓存失败' }, '操作失败'), '读缓存失败')
  assert.equal(errorMessage(new Error('boom'), '操作失败'), 'boom')
  assert.equal(errorMessage('nope', '操作失败'), '操作失败')
  assert.equal(errorMessage(null, '操作失败'), '操作失败')
})

test('formatBytes：按 1024 进制换算', () => {
  assert.equal(formatBytes(0), '0 B')
  assert.equal(formatBytes(-1), '0 B')
  assert.equal(formatBytes(512), '512 B')
  assert.equal(formatBytes(2048), '2.0 KB')
  assert.equal(formatBytes(1_267_641), '1.2 MB')
})

test('wholeDays：取整并夹住负数', () => {
  assert.equal(wholeDays(null), null)
  assert.equal(wholeDays(0.4), 0)
  assert.equal(wholeDays(6.6), 7)
  assert.equal(wholeDays(-1), 0)
})

test('splitLines：按行或逗号切分，去空白与重复', () => {
  assert.deepEqual(splitLines('  字典加速  \n建立字典,, 字典加速 '), ['字典加速', '建立字典'])
  assert.deepEqual(splitLines('   \n  '), [])
  assert.deepEqual(splitLines('单条'), ['单条'])
})

test('breadcrumbText：路径用斜杠连接', () => {
  assert.equal(breadcrumbText(['节点介绍', '执行节点']), '节点介绍 / 执行节点')
  assert.equal(breadcrumbText([]), '')
})

test('docPathId：只认官方详情页链接（应用内跳转要 path_id，不要地址）', () => {
  assert.equal(docPathId('https://act.mihoyo.com/ys/ugc/tutorial/detail/mh0abc123'), 'mh0abc123')
  assert.equal(docPathId('https://act.mihoyo.com/ys/ugc/tutorial/detail/mh0abc123/'), 'mh0abc123')
  assert.equal(docPathId('https://example.com/ys/ugc/tutorial/detail/x'), 'x')
  assert.equal(docPathId('https://act.mihoyo.com/ys/ugc/tutorial/list'), null)
  assert.equal(docPathId(''), null)
})

test('anchorId：加前缀，避免与页面自身的 id 撞车', () => {
  assert.equal(anchorId('NXQIOXpd'), 'ka-anchor-NXQIOXpd')
})

test('imageStyle：按官方宽度收窄，并给一个上限、不超容器', () => {
  assert.equal(imageStyle(420)?.maxWidth, 'min(420px, 100%)')
  assert.equal(imageStyle(5000)?.maxWidth, 'min(1200px, 100%)')
  assert.equal(imageStyle(null), undefined)
  assert.equal(imageStyle(0), undefined)
})
