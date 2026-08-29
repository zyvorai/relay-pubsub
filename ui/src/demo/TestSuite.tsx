// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

import { useState } from 'react'
import {
  API,
  PROJECT,
  ensureSubscription,
  ensureTopic,
  request,
  toBase64,
} from '../api/gateway'
import { CATALOGS } from '../catalogs'

type Status = 'idle' | 'run' | 'pass' | 'fail'

type Case = {
  id: string
  name: string
  status: Status
  ms?: number
  detail?: string
}

const INITIAL: Omit<Case, 'status'>[] = [
  { id: 'healthz', name: 'GET /healthz' },
  { id: 'readyz', name: 'GET /readyz' },
  { id: 'smoke', name: 'Smoke: publish → pull → ack' },
  { id: 'update', name: 'Update topic labels (PATCH)' },
  { id: 'snapshot', name: 'Snapshot + seek' },
  { id: 'page', name: 'List topics pagination' },
  { id: 'iam', name: 'IAM get / set policy' },
  { id: 'schema', name: 'Schema create + validate' },
  { id: 'push', name: 'Modify push config' },
]

async function timed<T>(fn: () => Promise<T>): Promise<{ value: T; ms: number }> {
  const t0 = performance.now()
  const value = await fn()
  return { value, ms: Math.round(performance.now() - t0) }
}

export function TestSuite() {
  const [cases, setCases] = useState<Case[]>(INITIAL.map((c) => ({ ...c, status: 'idle' })))
  const [busy, setBusy] = useState(false)

  const patch = (id: string, update: Partial<Case>) => {
    setCases((prev) => prev.map((c) => (c.id === id ? { ...c, ...update } : c)))
  }

  const runOne = async (id: string, fn: () => Promise<string>) => {
    patch(id, { status: 'run', detail: undefined, ms: undefined })
    try {
      const { value, ms } = await timed(fn)
      patch(id, { status: 'pass', ms, detail: value })
      return true
    } catch (e) {
      patch(id, { status: 'fail', detail: String(e) })
      return false
    }
  }

  const prepareResources = async (manageBusy = true) => {
    if (manageBusy) setBusy(true)
    try {
      const tests = CATALOGS.find((c) => c.id === 'tests')
      const demo = CATALOGS.find((c) => c.id === 'demo')
      for (const cat of [demo, tests]) {
        if (!cat) continue
        for (const short of cat.topics) {
          await ensureTopic(`${PROJECT}/topics/${short}`, { managedBy: 'relay-pubsub-ui' })
          if (cat.withSubs) {
            await ensureSubscription(
              `${PROJECT}/subscriptions/${short}-sub`,
              `${PROJECT}/topics/${short}`,
            )
          }
        }
        for (const s of cat.extraSubs || []) {
          await ensureTopic(`${PROJECT}/topics/${s.topic}`, { managedBy: 'relay-pubsub-ui' })
          await ensureSubscription(
            `${PROJECT}/subscriptions/${s.name}`,
            `${PROJECT}/topics/${s.topic}`,
          )
        }
      }
    } finally {
      if (manageBusy) setBusy(false)
    }
  }

  const runAll = async () => {
    setBusy(true)
    setCases(INITIAL.map((c) => ({ ...c, status: 'idle' })))
    await prepareResources(false)
    const topic = `${PROJECT}/topics/conformance-ui`
    const sub = `${PROJECT}/subscriptions/conformance-ui-sub`
    const snap = `${PROJECT}/snapshots/conformance-ui-snap`
    const schema = `${PROJECT}/schemas/conformance-ui-schema`

    await runOne('healthz', async () => {
      const r = await request<{ status: string }>('/healthz')
      if (r.status !== 'ok') throw new Error(JSON.stringify(r))
      return 'ok'
    })
    await runOne('readyz', async () => {
      const r = await request<{ status?: string; ready?: boolean }>('/readyz')
      return JSON.stringify(r).slice(0, 120)
    })

    await runOne('smoke', async () => {
      await ensureTopic(topic, { env: 'ui-smoke' })
      await ensureSubscription(sub, topic, { enableExactlyOnceDelivery: true })
      const data = toBase64(JSON.stringify({ order: 'ORD-UI-1' }))
      await request(`/v1/${topic}:publish`, {
        method: 'POST',
        body: JSON.stringify({
          messages: [{ data, attributes: { source: 'ui-suite' }, orderingKey: 'k1' }],
        }),
      })
      const pull = await request<{ receivedMessages: Array<{ ackId: string }> }>(`/v1/${sub}:pull`, {
        method: 'POST',
        body: JSON.stringify({ maxMessages: 5 }),
      })
      const ids = (pull.receivedMessages || []).map((m) => m.ackId)
      if (!ids.length) throw new Error('pull returned 0 messages')
      await request(`/v1/${sub}:acknowledge`, {
        method: 'POST',
        body: JSON.stringify({ ackIds: ids }),
      })
      return `acked ${ids.length}`
    })

    await runOne('update', async () => {
      const r = await request<{ labels?: Record<string, string> }>(
        `/v1/${topic}?updateMask=labels`,
        { method: 'PATCH', body: JSON.stringify({ labels: { t: '2', source: 'ui' } }) },
      )
      return JSON.stringify(r.labels || r).slice(0, 100)
    })

    await runOne('snapshot', async () => {
      try {
        await request(`/v1/${snap}`, { method: 'DELETE' })
      } catch {
        /* optional */
      }
      await request(`/v1/${snap}`, {
        method: 'PUT',
        body: JSON.stringify({ subscription: sub, labels: {} }),
      })
      await request(`/v1/${sub}:seek`, {
        method: 'POST',
        body: JSON.stringify({ snapshot: snap }),
      })
      return 'seek ok'
    })

    await runOne('page', async () => {
      for (let i = 1; i <= 3; i++) {
        await ensureTopic(`${PROJECT}/topics/page-ui-${i}`, {})
      }
      const page = await request<{ topics: unknown[]; nextPageToken?: string }>(
        `/v1/${PROJECT}/topics?pageSize=2`,
      )
      if (!page.nextPageToken && (page.topics?.length || 0) < 2) {
        throw new Error('expected nextPageToken or >=2 topics')
      }
      return `token=${page.nextPageToken || '(end)'} n=${page.topics?.length || 0}`
    })

    await runOne('iam', async () => {
      await request(`/v1/${topic}:getIamPolicy`, { method: 'POST', body: '{}' })
      await request(`/v1/${topic}:setIamPolicy`, {
        method: 'POST',
        body: JSON.stringify({
          policy: {
            bindings: [{ role: 'roles/pubsub.publisher', members: ['allAuthenticatedUsers'] }],
          },
        }),
      })
      return 'policy set'
    })

    await runOne('schema', async () => {
      try {
        await request(`/v1/${schema}`, { method: 'DELETE' })
      } catch {
        /* optional */
      }
      await request(`/v1/${schema}`, {
        method: 'PUT',
        body: JSON.stringify({
          type: 'AVRO',
          definition: '{"type":"record","name":"E","fields":[]}',
        }),
      })
      await request(`/v1/${PROJECT}/schemas:validate`, {
        method: 'POST',
        body: JSON.stringify({
          schema: {
            name: schema,
            type: 'AVRO',
            definition: '{"type":"record","name":"E","fields":[]}',
          },
        }),
      })
      return 'validated'
    })

    await runOne('push', async () => {
      await request(`/v1/${sub}:modifyPushConfig`, {
        method: 'POST',
        body: JSON.stringify({
          pushConfig: { pushEndpoint: 'https://example.invalid/push' },
        }),
      })
      return 'push config set'
    })

    setBusy(false)
  }

  const copyReport = async () => {
    const report = {
      api: API,
      project: PROJECT,
      at: new Date().toISOString(),
      cases,
    }
    await navigator.clipboard.writeText(JSON.stringify(report, null, 2))
  }

  const passed = cases.filter((c) => c.status === 'pass').length
  const failed = cases.filter((c) => c.status === 'fail').length

  return (
    <div className="suite-panel">
      <div className="suite-actions">
        <button className="btn btn-primary" onClick={runAll} disabled={busy}>
          {busy ? 'Running…' : 'Run all tests'}
        </button>
        <button className="btn btn-dark" onClick={() => prepareResources()} disabled={busy}>
          Prepare test resources
        </button>
        <button className="btn btn-ghost" onClick={copyReport} disabled={busy}>
          Copy report
        </button>
        <span style={{ fontSize: 13, color: 'var(--ink-secondary)', alignSelf: 'center' }}>
          {passed} passed · {failed} failed · {API || 'same-origin'}
        </span>
      </div>
      <div className="suite-list">
        {cases.map((c) => (
          <div key={c.id} className={`suite-row ${c.status}`}>
            <span className="badge">{c.status}</span>
            <span>{c.name}</span>
            <span className="ms">{c.ms != null ? `${c.ms} ms` : ''}</span>
            {c.detail && <div className="detail">{c.detail}</div>}
          </div>
        ))}
      </div>
    </div>
  )
}
