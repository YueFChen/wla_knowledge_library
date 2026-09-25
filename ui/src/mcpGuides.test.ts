import assert from 'node:assert/strict'
import { test } from 'node:test'

import { MCP_BINARY_PLACEHOLDER, mcpClientConfig } from './mcpGuides.ts'

test('通用与 Trae 生成标准 MCP JSON', () => {
  const config = JSON.parse(mcpClientConfig('trae', String.raw`C:\Program Files\Wonderland\mcp.exe`))
  assert.deepEqual(config.mcpServers['wonderland-knowledge'], {
    command: String.raw`C:\Program Files\Wonderland\mcp.exe`,
    args: [],
  })
  assert.doesNotThrow(() => JSON.parse(mcpClientConfig('generic', null)))
})

test('WorkBuddy 声明 stdio 类型', () => {
  const config = JSON.parse(mcpClientConfig('workbuddy', null))
  assert.equal(config.mcpServers['wonderland-knowledge'].type, 'stdio')
  assert.equal(config.mcpServers['wonderland-knowledge'].command, MCP_BINARY_PLACEHOLDER)
})

test('Codex 生成可合并的 TOML 段落并转义 Windows 路径', () => {
  const config = mcpClientConfig('codex', String.raw`C:\Wonderland\mcp-knowledge.exe`)
  assert.match(config, /^\[mcp_servers\.wonderland-knowledge\]/)
  assert.match(config, /command = "C:\\\\Wonderland\\\\mcp-knowledge\.exe"/)
  assert.match(config, /args = \[\]$/)
})
