import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { ChevronDown, ChevronUp, ArrowDown, Square } from 'lucide-react'
import type { Api, Job, LogBatch, LogEntry } from './workspaceTypes'

const ROW = 26
export function TaskLogPanel({ api, job, cancel }: { api: Api; job: Job; cancel: () => void }) {
  const [expanded, setExpanded] = useState(false)
  const [height, setHeight] = useState(220)
  const [entries, setEntries] = useState<LogEntry[]>([])
  const [top, setTop] = useState(0)
  const [following, setFollowing] = useState(true)
  const [error, setError] = useState('')
  const viewport = useRef<HTMLDivElement>(null)
  const follow = useRef(true)
  const running = job.state === 'running'
  const stillRunning = useRef(running)
  useEffect(() => {
    stillRunning.current = running
  }, [running])
  useEffect(() => {
    if (!job.id) return
    let active = true
    let timer: number | undefined
    let cursor = 0
    let idlePolls = 0
    setEntries([])
    setError('')
    follow.current = true
    setFollowing(true)
    const poll = async () => {
      try {
        const batch = await api.call<LogBatch>('task_logs', { job_id: job.id, cursor })
        if (!active || batch.job_id !== job.id) return
        cursor = batch.next_cursor
        if (batch.reset || batch.entries.length)
          setEntries((current) => (batch.reset ? batch.entries : [...current, ...batch.entries]))
        idlePolls = batch.entries.length ? 0 : idlePolls + 1
        setError('')
        if (batch.has_more || stillRunning.current || idlePolls < 2)
          timer = window.setTimeout(() => void poll(), batch.has_more ? 0 : 800)
      } catch (reason) {
        if (active) {
          setError(String(reason))
          timer = window.setTimeout(() => void poll(), 3000)
        }
      }
    }
    void poll()
    return () => {
      active = false
      window.clearTimeout(timer)
    }
  }, [api, job.id])
  useLayoutEffect(() => {
    if (expanded && follow.current && viewport.current) {
      viewport.current.scrollTop = viewport.current.scrollHeight
      setTop(viewport.current.scrollTop)
    }
  }, [entries.length, expanded, height])
  const resize = (event: React.PointerEvent<HTMLDivElement>) => {
    const startY = event.clientY
    const initial = height
    event.currentTarget.setPointerCapture(event.pointerId)
    const element = event.currentTarget
    const move = (moveEvent: PointerEvent) =>
      setHeight(
        Math.max(120, Math.min(window.innerHeight * 0.65, initial + startY - moveEvent.clientY)),
      )
    const stop = () => {
      element.removeEventListener('pointermove', move)
      element.removeEventListener('pointerup', stop)
      element.removeEventListener('pointercancel', stop)
    }
    element.addEventListener('pointermove', move)
    element.addEventListener('pointerup', stop)
    element.addEventListener('pointercancel', stop)
  }
  const start = Math.max(0, Math.floor(top / ROW) - 5)
  const end = Math.min(entries.length, start + Math.ceil(height / ROW) + 12)
  const state = running
    ? '任务进行中'
    : job.state === 'completed'
      ? '任务完成'
      : job.state === 'cancelled'
        ? '任务已取消'
        : '任务失败'
  return (
    <aside className="kb-task" aria-label="任务日志">
      {expanded ? (
        <div
          className="kb-log-resize"
          role="separator"
          aria-label="调整日志高度"
          aria-valuemin={120}
          aria-valuemax={Math.floor(window.innerHeight * 0.65)}
          aria-valuenow={Math.floor(height)}
          aria-orientation="horizontal"
          tabIndex={0}
          onPointerDown={resize}
          onKeyDown={(event) => {
            if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
              event.preventDefault()
              setHeight((value) =>
                Math.max(
                  120,
                  Math.min(window.innerHeight * 0.65, value + (event.key === 'ArrowUp' ? 26 : -26)),
                ),
              )
            }
          }}
        />
      ) : null}
      <div className="kb-task-summary">
        <button className="kb-link" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
          {expanded ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
          <strong>{state}</strong>
        </button>
        <span className="kb-task-current" title={job.current}>
          {job.current}
        </span>
        {job.total > 0 ? (
          <small>
            {job.done} / {job.total}
          </small>
        ) : null}
        {expanded && !following ? (
          <button
            className="kb-link"
            onClick={() => {
              follow.current = true
              setFollowing(true)
              viewport.current?.scrollTo({ top: viewport.current.scrollHeight })
            }}
          >
            <ArrowDown size={13} />
            跟随最新
          </button>
        ) : null}
        {running ? (
          <button className="kb-link" onClick={cancel}>
            <Square size={12} />
            取消
          </button>
        ) : null}
      </div>
      {running && job.total > 0 ? <progress max={job.total} value={job.done} /> : null}
      {expanded ? (
        <>
          {error ? (
            <p className="kb-error" role="alert">
              {error}
            </p>
          ) : null}
          <div
            className="kb-log-viewport"
            style={{ height }}
            ref={viewport}
            onScroll={(event) => {
              const element = event.currentTarget
              setTop(element.scrollTop)
              follow.current = element.scrollHeight - element.scrollTop - element.clientHeight < ROW
              setFollowing(follow.current)
            }}
          >
            <div style={{ height: entries.length * ROW, position: 'relative', minWidth: '100%' }}>
              {entries.slice(start, end).map((entry) => (
                <div
                  className={`kb-log-row ${entry.level}`}
                  key={entry.sequence}
                  style={{ top: entry.sequence * ROW, height: ROW }}
                  title={entry.message}
                >
                  <time>
                    {new Date(entry.time * 1000).toLocaleTimeString('zh-CN', { hour12: false })}
                  </time>
                  <span>
                    {entry.level === 'warning' ? '警告' : entry.level === 'error' ? '错误' : '信息'}
                  </span>
                  <span>{entry.message}</span>
                </div>
              ))}
            </div>
          </div>
        </>
      ) : null}
    </aside>
  )
}
