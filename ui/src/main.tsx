import { createRoot } from 'react-dom/client'
import { useEffect } from 'react'
import { createPluginHostClient } from '@wonderland/plugin-ui-sdk'
import { WorkspacePage } from './WorkspacePage'
import type { Api } from './workspaceTypes'
import './host.css'
const host = createPluginHostClient('wla_knowledge_library')
let activeRequest: string | null = null
const api: Api = {
  async call<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
    if (method === 'job_cancel' && activeRequest) host.cancel(activeRequest)
    if (method === 'build') {
      const request = host.callWithId<T>(method, params)
      activeRequest = request.requestId
      try {
        return await request.promise
      } finally {
        if (activeRequest === request.requestId) activeRequest = null
      }
    }
    return host.call<T>(method, params)
  },
}
function App() {
  useEffect(() => {
    let disposed = false
    let stopTheme = () => {}
    let stopLifecycle = () => {}
    void host
      .followHostTheme(({ resolved }) => {
        document.documentElement.dataset.theme = resolved
      })
      .then((stop) => {
        if (disposed) stop()
        else stopTheme = stop
      })
    void host
      .onSurfaceLifecycle((state) => {
        document.documentElement.dataset.surfaceState = state
      })
      .then((stop) => {
        if (disposed) stop()
        else stopLifecycle = stop
      })
    return () => {
      disposed = true
      stopTheme()
      stopLifecycle()
    }
  }, [])
  return <WorkspacePage api={api} />
}
createRoot(document.getElementById('root')!).render(<App />)
