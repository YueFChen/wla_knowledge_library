/**
 * 复制到剪贴板。
 *
 * 自定义协议（`tauri://localhost`）下 `navigator.clipboard` 可能拿不到（非安全上下文），
 * 所以失败后退回 `execCommand('copy')` + 临时 textarea：这是没装剪贴板插件时唯一还能用的路径。
 * 返回是否成功，由调用方给出对应文案。
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard) {
      await navigator.clipboard.writeText(text)
      return true
    }
  } catch {
    // 落到下面的兜底路径
  }
  try {
    const area = document.createElement('textarea')
    area.value = text
    area.setAttribute('readonly', '')
    area.style.position = 'fixed'
    area.style.opacity = '0'
    document.body.appendChild(area)
    area.select()
    const done = document.execCommand('copy')
    document.body.removeChild(area)
    return done
  } catch {
    return false
  }
}
