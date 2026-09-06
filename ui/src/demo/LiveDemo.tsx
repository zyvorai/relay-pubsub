// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

import { useState } from 'react'
import {
  API,
  PROJECT,
  decodeBytes,
  ensureSubscription,
  ensureTopic,
  request,
  shortName,
  toBase64,
  type Delivery,
} from '../api/gateway'

type Step = 'idle' | 'accept' | 'queue' | 'deliver' | 'ack' | 'done' | 'error'

function decodePayload(d: Delivery) {
  try {
    return decodeBytes(d.message.data)
  } catch {
    return JSON.stringify(d.message)
  }
}

const STEPS: { id: Step; title: string; body: string }[] = [
  { id: 'accept', title: 'Accept', body: 'Create topic & subscription' },
  { id: 'queue', title: 'Publish', body: 'Send a Pub/Sub message' },
  { id: 'deliver', title: 'Deliver', body: 'Pull from the subscription' },
  { id: 'ack', title: 'Acknowledge', body: 'Confirm delivery' },
]

export function LiveDemo() {
  const [step, setStep] = useState<Step>('idle')
  const [status, setStatus] = useState('Ready when you are.')
  const [busy, setBusy] = useState(false)
  const [payload, setPayload] = useState<string | null>(null)

  const run = async () => {
    setBusy(true)
    setPayload(null)
    setStatus('')
    const topic = `${PROJECT}/topics/demo.hello`
    const sub = `${PROJECT}/subscriptions/demo.hello-sub`
    try {
      setStep('accept')
      setStatus(`Preparing ${shortName(topic)}…`)
      await ensureTopic(topic, { demo: 'true' })
      await ensureSubscription(sub, topic)

      setStep('queue')
      const body = {
        hello: 'relay-pubsub',
        at: new Date().toISOString(),
        note: 'Live demo from the product console',
      }
      setStatus('Publishing…')
      const pub = await request<{ messageIds: string[] }>(`/v1/${topic}:publish`, {
        method: 'POST',
        body: JSON.stringify({
          messages: [
            {
              data: toBase64(JSON.stringify(body)),
              attributes: { source: 'live-demo', severity: 'info' },
              orderingKey: 'demo',
            },
          ],
        }),
      })
      setStatus(`Published ${pub.messageIds?.[0] || 'message'}`)

      setStep('deliver')
      setStatus('Pulling…')
      const pull = await request<{ receivedMessages: Array<{ ackId: string; message: { data: string; messageId: string } }> }>(
        `/v1/${sub}:pull`,
        { method: 'POST', body: JSON.stringify({ maxMessages: 5 }) },
      )
      const msgs = pull.receivedMessages || []
      if (msgs.length === 0) {
        // fallback admin pull for memory backends that used admin paths
        const admin = await request<Delivery[]>('/admin/v1/pull', {
          method: 'POST',
          body: JSON.stringify({ subscription: sub, max_messages: 5 }),
        })
        if (!admin.length) throw new Error('No messages received — is the gateway reachable?')
        setPayload(decodePayload(admin[0]))
        setStep('ack')
        await request('/admin/v1/ack', {
          method: 'POST',
          body: JSON.stringify({ subscription: sub, ack_ids: admin.map((d) => d.ack_id) }),
        })
      } else {
        try {
          setPayload(atob(msgs[0].message.data))
        } catch {
          setPayload(JSON.stringify(body, null, 2))
        }
        setStep('ack')
        await request(`/v1/${sub}:acknowledge`, {
          method: 'POST',
          body: JSON.stringify({ ackIds: msgs.map((m) => m.ackId) }),
        })
      }

      setStep('done')
      setStatus(`Demo complete against ${API}`)
    } catch (e) {
      setStep('error')
      setStatus(String(e))
    } finally {
      setBusy(false)
    }
  }

  const stepClass = (id: Step) => {
    if (step === 'error') return ''
    const order = STEPS.map((s) => s.id)
    const cur = order.indexOf(step === 'done' ? 'ack' : step)
    const mine = order.indexOf(id)
    if (step === 'done' || (cur > mine && mine >= 0)) return 'done'
    if (id === step) return 'active'
    return ''
  }

  return (
    <div className="demo-panel">
      <div className="timeline">
        {STEPS.map((s, i) => (
          <div key={s.id} className={`timeline-step ${stepClass(s.id)}`}>
            <div className="n">0{i + 1}</div>
            <h4>{s.title}</h4>
            <p>{s.body}</p>
          </div>
        ))}
      </div>
      <button className="btn btn-primary" onClick={run} disabled={busy}>
        {busy ? 'Running demo…' : 'Run live demo'}
      </button>
      <p className={`demo-status ${step === 'done' ? 'ok' : step === 'error' ? 'err' : ''}`}>
        {status}
      </p>
      {payload && (
        <pre style={{ marginTop: 16, fontSize: 12, background: 'var(--bg)', padding: 14, borderRadius: 12, overflow: 'auto' }}>
          {payload}
        </pre>
      )}
    </div>
  )
}
