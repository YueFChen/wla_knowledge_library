import assert from 'node:assert/strict'
import { test } from 'node:test'

import { MCP_BINARY_PLACEHOLDER, mcpClientConfig } from './mcpGuides.ts'

test('通用与 Trae 生成标准 MCP JSON', () => {
  const config = JSON.parse(
    mcpClientConfig('trae', String.raw`C:\Program Files\Wonderland\mcp.exe`),
  )
  assert.deepEqual(config.mcpServers['wla_knowledge_library'], {
    command: String.raw`C:\Program Files\Wonderland\mcp.exe`,
    args: [],
  })
  assert.doesNotThrow(() => JSON.parse(mcpClientConfig('generic', null)))
})

test('WorkBuddy 声明 stdio 类型', () => {
  const config = JSON.parse(mcpClientConfig('workbuddy', null))
  assert.equal(config.mcpServers['wla_knowledge_library'].type, 'stdio')
  assert.equal(config.mcpServers['wla_knowledge_library'].command, MCP_BINARY_PLACEHOLDER)
})

test('Codex 生成可合并的 TOML 段落并转义 Windows 路径', () => {
  const config = mcpClientConfig('codex', String.raw`C:\Wonderland\wla_knowledge_mcp.exe`)
  assert.match(config, /^\[mcp_servers\.wla_knowledge_library\]/)
  assert.match(config, /command = "C:\\\\Wonderland\\\\wla_knowledge_mcp\.exe"/)
  assert.match(config, /args = \[\]$/)
})

test('所有客户端保留实际数据目录，避免改名后误读旧库', () => {
  const cache = String.raw`D:\WLA Data\plugin-data\wla_knowledge_library`
  for (const client of ['generic', 'workbuddy', 'trae'] as const) {
    const config = JSON.parse(mcpClientConfig(client, null, cache))
    assert.deepEqual(config.mcpServers.wla_knowledge_library.args, ['--cache-dir', cache])
  }
  const args = mcpClientConfig('codex', null, cache)
    .split('\n')
    .find((line) => line.startsWith('args = '))!
  assert.deepEqual(JSON.parse(args.slice(7)), ['--cache-dir', cache])
})
