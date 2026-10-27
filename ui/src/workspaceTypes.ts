import type { PageElement } from './types.generated'

export interface Source {
  id: string
  name: string
  kind: 'files' | 'directory' | 'website' | 'wonderland'
  paths: string[]
  url: string
  path_prefix: string
  depth: number
  max_pages: number
  dynamic: boolean
}
export interface Library {
  id: string
  name: string
  preset_id: string
  sources: Source[]
  exposed: boolean
  model: string | null
  model_requirement?: ModelRequirement | null
  revision: string
  snapshot: string | null
  vector_revision: string | null
  created_at: number
  updated_at: number
}
export interface Preset {
  id: string
  name: string
  sources: Source[]
}
export interface Service {
  id: string
  name: string
  kind: 'ollama' | 'openai'
  url: string
  secret_id?: string | null
}
export interface ModelRequirement {
  name: string
  model: string
  dimensions: number
  max_chars: number
  query_prefix: string
  document_prefix: string
}
export interface Model extends ModelRequirement {
  id: string
  service_id: string
}
export interface LogEntry {
  sequence: number
  time: number
  level: string
  message: string
}
export interface LogBatch {
  job_id: string
  entries: LogEntry[]
  next_cursor: number
  has_more: boolean
  reset: boolean
}
export interface Outline {
  anchor: string
  text: string
  level: number
  index: number
}
export interface Job {
  id: string
  kind: string
  library_id: string | null
  state: string
  done: number
  total: number
  current: string
  errors: string[]
  started_at: number
  ended_at: number | null
}
export interface Status {
  libraries: Library[]
  presets: Preset[]
  models: Model[]
  services: Service[]
  job: Job
  cache_dir: string
  edge_available: boolean
  version: string
  mcp_config: { mcpServers: { wla_knowledge_library: { command: string; args: string[] } } }
}
export interface Brief {
  id: string
  source_id: string
  title: string
  url: string
  updated_at: number
  library_id: string
  library_name: string
  breadcrumb: string[]
  section: string
}
export interface Hit {
  library_id: string
  library_name: string
  path_id: string
  doc_title: string
  anchor: string
  text: string
  url: string
  breadcrumb: string[]
  kind: string
}
export interface Document {
  outline: Outline[]
  next_offset: number | null
  element_offset: number
  total_elements: number
  id: string
  source_id: string
  title: string
  url: string
  updated_at: number
  elements: PageElement[]
  metadata: { breadcrumb: string[] }
}
export interface Api {
  call<T = unknown>(method: string, params?: Record<string, unknown>): Promise<T>
}
