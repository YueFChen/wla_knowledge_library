export const MCP_SERVER_NAME = 'wla_knowledge_library'
export const MCP_BINARY_PLACEHOLDER = String.raw`C:\path\to\wla_knowledge_mcp.exe`

export type McpClient = 'generic' | 'codex' | 'workbuddy' | 'trae'

function commandOf(binary: string | null): string {
  return binary ?? MCP_BINARY_PLACEHOLDER
}

function jsonConfig(command: string, args: string[], extra: Record<string, unknown> = {}): string {
  return JSON.stringify(
    {
      mcpServers: {
        [MCP_SERVER_NAME]: {
          ...extra,
          command,
          args,
        },
      },
    },
    null,
    2,
  )
}

/**
 * 各客户端最终都启动同一个 stdio 程序；这里只适配它们的配置载体，避免后端
 * 为纯展示格式维护多份字符串。JSON.stringify 同时负责 Windows 路径转义。
 */
export function mcpClientConfig(
  client: McpClient,
  binary: string | null,
  cacheDir?: string,
): string {
  const command = commandOf(binary)
  const args = cacheDir ? ['--cache-dir', cacheDir] : []

  if (client === 'codex') {
    return [
      `[mcp_servers.${MCP_SERVER_NAME}]`,
      `command = ${JSON.stringify(command)}`,
      `args = ${JSON.stringify(args)}`,
    ].join('\n')
  }

  if (client === 'workbuddy') {
    return jsonConfig(command, args, { type: 'stdio' })
  }

  return jsonConfig(command, args)
}

export const MCP_CLIENTS: readonly McpClient[] = ['generic', 'codex', 'workbuddy', 'trae']
