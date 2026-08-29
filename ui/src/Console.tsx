// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  API,
  PROJECT,
  decodeBytes,
  fetchInventory,
  fetchLogs,
  request,
  shortName,
  type Delivery,
  type InventoryReport,
  type LogEntry as GatewayLogEntry,
  type SubscriptionInventory,
} from './api/gateway'

type Pane = 'incoming' | 'outgoing' | 'stored' | 'configure' | 'logs'
type LogEntry = { id: number; ts: number; kind: 'info' | 'success' | 'error'; text: string }

const AUTO_REFRESH_MS = 4000
const LIVE_RECEIVE_MS = 3000
const LOGS_POLL_MS = 2000

let logSeq = 0

export function Console() {
  const [pane, setPane] = useState<Pane>('stored')
  const [inventory, setInventory] = useState<InventoryReport>({ topics: [], subscriptions: [] })
  const [selectedTopic, setSelectedTopic] = useState('')
  const [selectedSub, setSelectedSub] = useState('')
  const [payload, setPayload] = useState('{"order_id":"ORD-1001","status":"created"}')
  const [orderingKey, setOrderingKey] = useState('demo')
  const [deliveries, setDeliveries] = useState<Delivery[]>([])
  const [topicName, setTopicName] = useState('orders')
  const [subName, setSubName] = useState('orders-worker')
  const [pushEndpoint, setPushEndpoint] = useState('')
  const [ackDeadline, setAckDeadline] = useState(20)
  const [status, setStatus] = useState('Ready')
  const [busy, setBusy] = useState(false)
  const [connected, setConnected] = useState(false)
  const [liveReceive, setLiveReceive] = useState(false)
  const [log, setLog] = useState<LogEntry[]>([])
  const [peekTopicName, setPeekTopicName] = useState('')
  const [gatewayLogs, setGatewayLogs] = useState<GatewayLogEntry[]>([])
  const [logsLive, setLogsLive] = useState(true)
  const [logsLevel, setLogsLevel] = useState('ALL')
  const [logsFilter, setLogsFilter] = useState('')
  const logsAfterRef = useRef(0)
  const logsScrollRef = useRef<HTMLDivElement>(null)
  const stickBottomRef = useRef(true)
  const seenMessageIds = useRef<Set<string>>(new Set())

  const logEvent = useCallback((kind: LogEntry['kind'], text: string) => {
    setLog((prev) => [{ id: ++logSeq, ts: Date.now(), kind, text }, ...prev].slice(0, 200))
  }, [])

  const refresh = useCallback(
    async (silent = false) => {
      try {
        const report = await fetchInventory()
        setInventory(report)
        if (!selectedTopic && report.topics[0]) setSelectedTopic(report.topics[0].name)
        if (!selectedSub && report.subscriptions[0]) setSelectedSub(report.subscriptions[0].name)
        setStatus('Connected to Relay gateway')
        setConnected((prev) => {
          if (!prev && !silent) {
            logEvent(
              'success',
              `Connected — ${report.topics.length} topics, ${report.subscriptions.length} subscriptions`,
            )
          }
          return true
        })
      } catch (e) {
        const hint =
          String(e).includes('Load failed') || String(e).includes('Failed to fetch')
            ? ' (hard-refresh the page — API is same-origin via /healthz proxy)'
            : ''
        setStatus(`Unavailable · ${String(e)}${hint}`)
        setConnected((prev) => {
          if (prev) logEvent('error', `Lost connection: ${String(e)}`)
          return false
        })
      }
    },
    [selectedSub, selectedTopic, logEvent],
  )

  const peekTopic = inventory.topics.find((t) => t.name === peekTopicName) || null

  const pollLogs = useCallback(
    async (reset = false) => {
      try {
        const after = reset ? 0 : logsAfterRef.current
        const res = await fetchLogs(after, reset ? 300 : 100)
        if (reset) {
          setGatewayLogs(res.entries)
        } else if (res.entries.length > 0) {
          setGatewayLogs((prev) => {
            const seen = new Set(prev.map((e) => e.id))
            const merged = [...prev]
            for (const e of res.entries) {
              if (!seen.has(e.id)) merged.push(e)
            }
            return merged.slice(-1000)
          })
        }
        if (res.next_after) logsAfterRef.current = res.next_after
      } catch (e) {
        if (reset) logEvent('error', `Logs unavailable: ${String(e)}`)
      }
    },
    [logEvent],
  )

  useEffect(() => {
    refresh()
  }, [refresh])

  useEffect(() => {
    const id = setInterval(() => refresh(true), AUTO_REFRESH_MS)
    return () => clearInterval(id)
  }, [refresh])

  useEffect(() => {
    if (pane !== 'logs') return
    void pollLogs(logsAfterRef.current === 0)
  }, [pane, pollLogs])

  useEffect(() => {
    if (pane !== 'logs' || !logsLive) return
    const id = setInterval(() => void pollLogs(false), LOGS_POLL_MS)
    return () => clearInterval(id)
  }, [pane, logsLive, pollLogs])

  useEffect(() => {
    if (pane !== 'logs' || !stickBottomRef.current) return
    const el = logsScrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [gatewayLogs, pane])

  useEffect(() => {
    const sub = inventory.subscriptions.find((s) => s.name === selectedSub)
    if (sub) setPushEndpoint(sub.push_endpoint || '')
  }, [selectedSub, inventory.subscriptions])

  const filteredLogs = gatewayLogs.filter((e) => {
    if (logsLevel !== 'ALL' && e.level !== logsLevel) return false
    if (!logsFilter.trim()) return true
    const q = logsFilter.toLowerCase()
    return (
      e.message.toLowerCase().includes(q) ||
      e.target.toLowerCase().includes(q) ||
      e.level.toLowerCase().includes(q)
    )
  })

  const createTopic = async () => {
    if (!topicName.trim()) return
    setBusy(true)
    try {
      const name = `${PROJECT}/topics/${topicName.trim()}`
      await request(`/v1/${name}`, {
        method: 'PUT',
        body: JSON.stringify({ labels: { managedBy: 'relay-pubsub-ui' } }),
      })
      setSelectedTopic(name)
      logEvent('success', `Created topic ${shortName(name)}`)
      await refresh()
    } catch (e) {
      logEvent('error', `Create topic failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const createSubscription = async () => {
    if (!subName.trim() || !selectedTopic) return
    setBusy(true)
    try {
      const name = `${PROJECT}/subscriptions/${subName.trim()}`
      await request(`/v1/${name}`, {
        method: 'PUT',
        body: JSON.stringify({
          topic: selectedTopic,
          ackDeadlineSeconds: ackDeadline,
          enableMessageOrdering: true,
          ...(pushEndpoint.trim()
            ? { pushConfig: { pushEndpoint: pushEndpoint.trim() } }
            : {}),
        }),
      })
      setSelectedSub(name)
      logEvent('success', `Created subscription ${shortName(name)}`)
      await refresh()
    } catch (e) {
      logEvent('error', `Create subscription failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const deleteResource = async (name: string, kind: 'topic' | 'subscription') => {
    if (!confirm(`Delete ${kind} ${shortName(name)}?`)) return
    setBusy(true)
    try {
      await request(`/v1/${name}`, { method: 'DELETE' })
      logEvent('info', `Deleted ${kind} ${shortName(name)}`)
      if (kind === 'topic' && selectedTopic === name) setSelectedTopic('')
      if (kind === 'subscription' && selectedSub === name) setSelectedSub('')
      await refresh()
    } catch (e) {
      logEvent('error', `Delete failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const savePushConfig = async () => {
    if (!selectedSub) return
    setBusy(true)
    try {
      await request('/admin/v1/push-config', {
        method: 'POST',
        body: JSON.stringify({
          subscription: selectedSub,
          push_endpoint: pushEndpoint.trim() || null,
        }),
      })
      logEvent(
        'success',
        pushEndpoint.trim()
          ? `Push → ${pushEndpoint.trim()} on ${shortName(selectedSub)}`
          : `Push cleared on ${shortName(selectedSub)} (pull only)`,
      )
      await refresh()
    } catch (e) {
      logEvent('error', `Push config failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const publish = async () => {
    if (!selectedTopic) return
    setBusy(true)
    try {
      const result = await request<{ message_ids: string[] }>('/admin/v1/publish', {
        method: 'POST',
        body: JSON.stringify({
          topic: selectedTopic,
          data: payload,
          attributes: { source: 'relay-console', direction: 'incoming' },
          ordering_key: orderingKey,
        }),
      })
      logEvent('success', `IN → ${shortName(selectedTopic)}: ${result.message_ids[0]}`)
      setStatus(`Published ${result.message_ids[0]}`)
      setPane('stored')
      await refresh()
    } catch (e) {
      logEvent('error', `Publish failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const pull = useCallback(
    async (silent = false) => {
      if (!selectedSub) return
      if (!silent) setBusy(true)
      try {
        const result = await request<Delivery[]>('/admin/v1/pull', {
          method: 'POST',
          body: JSON.stringify({ subscription: selectedSub, max_messages: 20 }),
        })
        const fresh = result.filter((d) => !seenMessageIds.current.has(d.message.id))
        fresh.forEach((d) => {
          seenMessageIds.current.add(d.message.id)
          logEvent('info', `OUT ← ${shortName(selectedSub)}: ${d.message.id}`)
        })
        if (fresh.length > 0) setDeliveries((prev) => [...fresh, ...prev].slice(0, 100))
        if (!silent) setStatus(`Pulled ${result.length} (${fresh.length} new)`)
        await refresh(true)
      } catch (e) {
        if (!silent) logEvent('error', `Pull failed: ${String(e)}`)
      } finally {
        if (!silent) setBusy(false)
      }
    },
    [selectedSub, logEvent, refresh],
  )

  useEffect(() => {
    if (!liveReceive || !selectedSub) return
    const id = setInterval(() => pull(true), LIVE_RECEIVE_MS)
    return () => clearInterval(id)
  }, [liveReceive, selectedSub, pull])

  const ackAll = async () => {
    if (!selectedSub || deliveries.length === 0) return
    setBusy(true)
    try {
      await request('/admin/v1/ack', {
        method: 'POST',
        body: JSON.stringify({
          subscription: selectedSub,
          ack_ids: deliveries.map((d) => d.ack_id),
        }),
      })
      logEvent('info', `Acked ${deliveries.length} on ${shortName(selectedSub)}`)
      setDeliveries([])
      setStatus('Acknowledged')
      await refresh()
    } catch (e) {
      logEvent('error', `Ack failed: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const totals = {
    topics: inventory.topics.length,
    subs: inventory.subscriptions.length,
    stored: inventory.topics.reduce((n, t) => n + t.message_count, 0),
    backlog: inventory.subscriptions.reduce((n, s) => n + s.backlog, 0),
    push: inventory.subscriptions.filter((s) => s.push_endpoint).length,
    inflight: inventory.subscriptions.reduce((n, s) => n + s.inflight, 0),
  }

  const selectedSubInv: SubscriptionInventory | undefined = inventory.subscriptions.find(
    (s) => s.name === selectedSub,
  )

  return (
    <div className="console-shell">
      <div className="console-meta">
        <div>
          Gateway{' '}
          <strong style={{ color: connected ? 'var(--success)' : 'var(--danger)' }}>
            {connected ? 'Connected' : 'Unreachable'}
          </strong>
        </div>
        <div>
          API <strong>{API || '(same origin / proxy)'}</strong>
        </div>
        <div>
          Project <strong>{PROJECT}</strong>
        </div>
        <div>
          Status <strong>{status}</strong>
        </div>
        <button className="btn btn-ghost" type="button" onClick={() => refresh()} disabled={busy}>
          Refresh
        </button>
      </div>

      <div className="flow-stats">
        <div className="flow-stat">
          <span>Stored</span>
          <strong>{totals.stored}</strong>
          <small>{totals.topics} topics</small>
        </div>
        <div className="flow-stat">
          <span>Backlog</span>
          <strong>{totals.backlog}</strong>
          <small>{totals.subs} subscriptions</small>
        </div>
        <div className="flow-stat">
          <span>Inflight</span>
          <strong>{totals.inflight}</strong>
          <small>leased now</small>
        </div>
        <div className="flow-stat">
          <span>Push</span>
          <strong>{totals.push}</strong>
          <small>outgoing endpoints</small>
        </div>
      </div>

      <div className="pane-tabs" role="tablist">
        {(
          [
            ['incoming', 'Incoming'],
            ['outgoing', 'Outgoing'],
            ['stored', 'Stored'],
            ['configure', 'Configure'],
            ['logs', 'Logs'],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={pane === id}
            className={`pane-tab${pane === id ? ' active' : ''}`}
            onClick={() => setPane(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {pane === 'incoming' && (
        <div className="pane-body">
          <p className="pane-lead">
            Publish into a topic — that is inbound traffic. Messages land in storage immediately.
          </p>
          <div className="console-grid">
            <div>
              <div className="field">
                <label>Topic (incoming)</label>
                <select value={selectedTopic} onChange={(e) => setSelectedTopic(e.target.value)}>
                  <option value="">Select topic</option>
                  {inventory.topics.map((t) => (
                    <option key={t.name} value={t.name}>
                      {shortName(t.name)} · {t.message_count} stored
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label>Ordering key</label>
                <input value={orderingKey} onChange={(e) => setOrderingKey(e.target.value)} />
              </div>
            </div>
            <div>
              <div className="field">
                <label>Payload</label>
                <textarea rows={6} value={payload} onChange={(e) => setPayload(e.target.value)} />
              </div>
              <button
                className="btn btn-primary"
                style={{ width: '100%', marginTop: 8 }}
                onClick={publish}
                disabled={busy || !selectedTopic}
              >
                Publish incoming
              </button>
            </div>
          </div>
        </div>
      )}

      {pane === 'outgoing' && (
        <div className="pane-body">
          <p className="pane-lead">
            Pull or push delivers stored messages out. Configure a push URL, or pull live into this
            console.
          </p>
          <div className="field">
            <label>Subscription (outgoing)</label>
            <select value={selectedSub} onChange={(e) => setSelectedSub(e.target.value)}>
              <option value="">Select subscription</option>
              {inventory.subscriptions.map((s) => (
                <option key={s.name} value={s.name}>
                  {shortName(s.name)} · backlog {s.backlog}
                  {s.push_endpoint ? ' · push' : ' · pull'}
                </option>
              ))}
            </select>
          </div>

          {selectedSubInv && (
            <div className="sub-out-meta">
              <span>
                Topic <strong>{shortName(selectedSubInv.topic)}</strong>
              </span>
              <span>
                Backlog <strong>{selectedSubInv.backlog}</strong>
              </span>
              <span>
                Inflight <strong>{selectedSubInv.inflight}</strong>
              </span>
              <span>
                Acked <strong>{selectedSubInv.acked}</strong>
              </span>
              <span>
                Mode{' '}
                <strong>{selectedSubInv.push_endpoint ? 'Push' : 'Pull'}</strong>
              </span>
            </div>
          )}

          <div className="field">
            <label>Push endpoint (optional — leave empty for pull)</label>
            <div className="inline-row">
              <input
                value={pushEndpoint}
                onChange={(e) => setPushEndpoint(e.target.value)}
                placeholder="https://worker.example/push"
                disabled={!selectedSub}
              />
              <button
                className="btn btn-dark"
                type="button"
                onClick={savePushConfig}
                disabled={busy || !selectedSub}
              >
                Save push
              </button>
            </div>
          </div>

          <div className="consumer-row">
            <button className="btn btn-dark" onClick={() => pull()} disabled={busy || !selectedSub}>
              Pull now
            </button>
            <label className="live-toggle">
              <input
                type="checkbox"
                checked={liveReceive}
                onChange={(e) => setLiveReceive(e.target.checked)}
                disabled={!selectedSub}
              />
              Live ({LIVE_RECEIVE_MS / 1000}s)
            </label>
            <button
              className="btn btn-success"
              onClick={ackAll}
              disabled={busy || deliveries.length === 0}
            >
              Ack all ({deliveries.length})
            </button>
          </div>

          <div className="message-list">
            {deliveries.length === 0 ? (
              <div className="empty">No outgoing deliveries pulled yet.</div>
            ) : (
              deliveries.map((d) => (
                <article key={d.ack_id} className="message">
                  <div>
                    <span className="chip">attempt {d.delivery_attempt}</span>
                    <b>{d.message.id}</b>
                    <small>{new Date(d.message.published_at).toLocaleString()}</small>
                  </div>
                  <pre>{decodeBytes(d.message.data)}</pre>
                </article>
              ))
            )}
          </div>
        </div>
      )}

      {pane === 'stored' && (
        <div className="pane-body">
          <p className="pane-lead">
            What the gateway holds right now — topics with message counts, and subscription cursors /
            backlog. Click a topic to peek recent payloads without consuming them.
          </p>
          <div className="stored-grid">
            <div>
              <h3 className="stored-heading">Topics</h3>
              {inventory.topics.length === 0 ? (
                <div className="empty">Nothing stored. Configure a topic or generate catalogs.</div>
              ) : (
                <div className="stored-list">
                  {inventory.topics.map((t) => (
                    <button
                      key={t.name}
                      type="button"
                      className={`stored-row${peekTopic?.name === t.name ? ' active' : ''}`}
                      onClick={() => setPeekTopicName(t.name)}
                    >
                      <div>
                        <strong>{shortName(t.name)}</strong>
                        <small>{t.name}</small>
                      </div>
                      <span className="stored-count">{t.message_count}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
            <div>
              <h3 className="stored-heading">Subscriptions</h3>
              {inventory.subscriptions.length === 0 ? (
                <div className="empty">No subscriptions.</div>
              ) : (
                <div className="stored-list">
                  {inventory.subscriptions.map((s) => (
                    <div key={s.name} className="stored-row static">
                      <div>
                        <strong>{shortName(s.name)}</strong>
                        <small>
                          ← {shortName(s.topic)}
                          {s.push_endpoint ? ` · push ${s.push_endpoint}` : ' · pull'}
                        </small>
                      </div>
                      <div className="stored-metrics">
                        <span title="Backlog">{s.backlog} wait</span>
                        <span title="Inflight">{s.inflight} out</span>
                        <span title="Acked">{s.acked} done</span>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>

          {peekTopic && (
            <div className="peek-panel">
              <div className="peek-head">
                <h3>
                  Stored on <em>{shortName(peekTopic.name)}</em>
                </h3>
                <span>{peekTopic.message_count} messages · newest first</span>
              </div>
              {peekTopic.recent.length === 0 ? (
                <div className="empty">Topic is empty.</div>
              ) : (
                <div className="message-list">
                  {peekTopic.recent.map((m) => (
                    <article key={m.id} className="message">
                      <div>
                        <span className="chip">stored</span>
                        <b>{m.id}</b>
                        {m.ordering_key ? <span className="chip muted">{m.ordering_key}</span> : null}
                        <small>{new Date(m.published_at).toLocaleString()}</small>
                      </div>
                      <pre>{m.data}</pre>
                    </article>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {pane === 'configure' && (
        <div className="pane-body">
          <p className="pane-lead">
            Create topics and subscriptions, wire push endpoints, or remove resources.
          </p>
          <div className="console-grid">
            <div>
              <div className="field">
                <label>New topic</label>
                <div className="inline-row">
                  <input
                    value={topicName}
                    onChange={(e) => setTopicName(e.target.value)}
                    placeholder="orders"
                  />
                  <button className="btn btn-dark" onClick={createTopic} disabled={busy}>
                    Create
                  </button>
                </div>
              </div>
              <div className="field">
                <label>Existing topics</label>
                <div className="config-list">
                  {inventory.topics.map((t) => (
                    <div key={t.name} className="config-row">
                      <span>{shortName(t.name)}</span>
                      <button
                        type="button"
                        className="btn btn-ghost"
                        onClick={() => deleteResource(t.name, 'topic')}
                        disabled={busy}
                      >
                        Delete
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            </div>
            <div>
              <div className="field">
                <label>Subscribe to topic</label>
                <select value={selectedTopic} onChange={(e) => setSelectedTopic(e.target.value)}>
                  <option value="">Select topic</option>
                  {inventory.topics.map((t) => (
                    <option key={t.name} value={t.name}>
                      {shortName(t.name)}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label>New subscription</label>
                <input
                  value={subName}
                  onChange={(e) => setSubName(e.target.value)}
                  placeholder="orders-worker"
                />
              </div>
              <div className="field">
                <label>Ack deadline (seconds)</label>
                <input
                  type="number"
                  min={10}
                  max={600}
                  value={ackDeadline}
                  onChange={(e) => setAckDeadline(Number(e.target.value) || 20)}
                />
              </div>
              <div className="field">
                <label>Push endpoint (optional)</label>
                <input
                  value={pushEndpoint}
                  onChange={(e) => setPushEndpoint(e.target.value)}
                  placeholder="https://… or leave blank for pull"
                />
              </div>
              <button
                className="btn btn-primary"
                style={{ width: '100%' }}
                onClick={createSubscription}
                disabled={busy || !selectedTopic || !subName.trim()}
              >
                Create subscription
              </button>
              <div className="field" style={{ marginTop: 18 }}>
                <label>Existing subscriptions</label>
                <div className="config-list">
                  {inventory.subscriptions.map((s) => (
                    <div key={s.name} className="config-row">
                      <span>
                        {shortName(s.name)}
                        <small>
                          {' '}
                          → {shortName(s.topic)}
                          {s.push_endpoint ? ' · push' : ''}
                        </small>
                      </span>
                      <button
                        type="button"
                        className="btn btn-ghost"
                        onClick={() => deleteResource(s.name, 'subscription')}
                        disabled={busy}
                      >
                        Delete
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {pane === 'logs' && (
        <div className="pane-body">
          <p className="pane-lead">
            Live gateway process logs (in-memory ring buffer). Publish, pull, or configure to generate
            new lines — journal noise from HTTP access is filtered out.
          </p>
          <div className="logs-toolbar">
            <label className="live-toggle">
              <input
                type="checkbox"
                checked={logsLive}
                onChange={(e) => setLogsLive(e.target.checked)}
              />
              Live ({LOGS_POLL_MS / 1000}s)
            </label>
            <select value={logsLevel} onChange={(e) => setLogsLevel(e.target.value)}>
              <option value="ALL">All levels</option>
              <option value="ERROR">ERROR</option>
              <option value="WARN">WARN</option>
              <option value="INFO">INFO</option>
              <option value="DEBUG">DEBUG</option>
            </select>
            <input
              className="logs-filter"
              value={logsFilter}
              onChange={(e) => setLogsFilter(e.target.value)}
              placeholder="Filter message or target…"
            />
            <button
              className="btn btn-ghost"
              type="button"
              onClick={() => {
                logsAfterRef.current = 0
                void pollLogs(true)
              }}
            >
              Reload
            </button>
            <button
              className="btn btn-ghost"
              type="button"
              onClick={() => {
                setGatewayLogs([])
                stickBottomRef.current = true
              }}
            >
              Clear view
            </button>
          </div>
          <div
            className="logs-viewer"
            ref={logsScrollRef}
            onScroll={(e) => {
              const el = e.currentTarget
              stickBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48
            }}
          >
            {filteredLogs.length === 0 ? (
              <div className="empty">No log lines yet. Trigger publish/pull or wait for startup events.</div>
            ) : (
              filteredLogs.map((e) => (
                <div key={e.id} className={`log-line level-${e.level.toLowerCase()}`}>
                  <span className="log-ts">{new Date(e.ts).toLocaleTimeString()}</span>
                  <span className="log-level">{e.level}</span>
                  <span className="log-target" title={e.target}>
                    {e.target.replace(/^relay_pubsub::/, '')}
                  </span>
                  <span className="log-msg">{e.message}</span>
                </div>
              ))
            )}
          </div>
        </div>
      )}

      <div>
        <div className="section-kicker" style={{ marginBottom: 8 }}>
          Activity
        </div>
        <div className="activity-list">
          {log.length === 0 ? (
            <div className="empty">Publish, pull, or configure to see activity.</div>
          ) : (
            log.map((e) => (
              <div key={e.id} className={`activity-row ${e.kind}`}>
                <small>{new Date(e.ts).toLocaleTimeString()}</small>
                <span>{e.text}</span>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  )
}
