import { createRoot } from 'react-dom/client'
import { useEffect } from 'react'
import { createPluginHostClient } from '@wonderland/plugin-ui-sdk'
import { KnowledgeLibraryPage } from './index'
import type { KnowledgeApi } from './api'
import type { MirrorProgress, MirrorRun } from './types.generated'
import './host.css'

const host = createPluginHostClient('knowledge_library')
let activeMirrorRequest: string | null = null

const api: KnowledgeApi = {
  status: () => host.call('status'),
  embeddingStatus: () => host.call('embedding_status'),
  activateEmbeddings: () => host.call('activate_embeddings'),
  disableEmbeddings: () => host.call('disable_embeddings'),
  recheckEmbeddings: () => host.call('recheck_embeddings'),
  refresh: () => host.call('refresh'),
  search: (queries, topK) => host.call('search', { queries, top_k: topK }),
  docs: keywords => host.call('docs', { keywords }),
  openSource: async pathId => { await host.call('open_source', { path_id: pathId }) },
  openModelSource: async source => { await host.call('open_model_source', { source }) },
  revealCacheDir: async () => { await host.call('reveal_cache_dir') },
  mirrorStatus: () => host.call('mirror_status'),
  mirrorStart: async (limit, onProgress) => {
    await host.ready
    let requestId = ''
    const stop = await host.subscribe('mirror.progress', event => { if (event.requestId === requestId) onProgress(event.payload as MirrorProgress) })
    const call = host.callWithId<MirrorRun>('mirror_start', { limit })
    requestId = call.requestId
    activeMirrorRequest = requestId
    try { return await call.promise }
    finally { stop(); if (activeMirrorRequest === requestId) activeMirrorRequest = null }
  },
  mirrorCancel: async () => { if (activeMirrorRequest) host.cancel(activeMirrorRequest) },
  page: pathId => host.call('page', { path_id: pathId }),
  mirrorPage: pathId => host.call('mirror_page', { path_id: pathId }),
}

function App() {
  useEffect(() => {
    let disposed = false
    let stopTheme = () => {}
    let stopLifecycle = () => {}
    void host.followHostTheme(({ resolved }) => { document.documentElement.dataset.theme = resolved })
      .then(stop => { if (disposed) stop(); else stopTheme = stop })
    void host.onSurfaceLifecycle(state => { document.documentElement.dataset.surfaceState = state })
      .then(stop => { if (disposed) stop(); else stopLifecycle = stop })
    return () => { disposed = true; stopTheme(); stopLifecycle() }
  }, [])
  return <KnowledgeLibraryPage api={api} />
}

createRoot(document.getElementById('root')!).render(<App />)
