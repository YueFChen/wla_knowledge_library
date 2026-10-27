export function documentUrlKey(value: string): string | null {
  try {
    const url = new URL(value)
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return null
    // Older official snapshots contain tutorial//detail links for the same document.
    const path =
      url.hostname === 'act.mihoyo.com' ? url.pathname.replace(/\/{2,}/g, '/') : url.pathname
    return `${url.origin}${path}${url.search}`
  } catch {
    return null
  }
}
