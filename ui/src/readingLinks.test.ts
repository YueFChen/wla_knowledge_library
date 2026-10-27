import assert from 'node:assert/strict'
import test from 'node:test'
import { documentUrlKey } from './readingLinks.ts'

test('旧千星链接的重复斜杠与锚点仍指向同一库内文档', () => {
  assert.equal(
    documentUrlKey('https://act.mihoyo.com/ys/ugc/tutorial//detail/abc#section'),
    documentUrlKey('https://act.mihoyo.com/ys/ugc/tutorial/detail/abc'),
  )
})
test('普通站点保留查询和路径差异，不将其他资料误匹配成当前文档', () => {
  assert.notEqual(
    documentUrlKey('https://example.com/a?doc=1'),
    documentUrlKey('https://example.com/a?doc=2'),
  )
  assert.notEqual(documentUrlKey('https://example.com//a'), documentUrlKey('https://example.com/a'))
  assert.equal(documentUrlKey('javascript:alert(1)'), null)
})
