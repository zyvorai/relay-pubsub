import { useCallback, useEffect, useMemo, useState } from 'react'
import { Activity, ArrowDownToLine, ArrowUpFromLine, CheckCircle2, CircleDot, DatabaseZap, Plus, RefreshCw, RadioTower, Send, ServerCog } from 'lucide-react'

type Topic = { name: string; labels: Record<string,string>; kms_key_name?: string }
type Subscription = { name: string; topic: string; ack_deadline_seconds: number; enable_message_ordering: boolean; enable_exactly_once_delivery: boolean }
type Delivery = { ack_id: string; delivery_attempt: number; message: { id: string; data: number[]; attributes: Record<string,string>; ordering_key: string; published_at: string } }

const API = import.meta.env.VITE_API_BASE || 'http://localhost:8080'
const PROJECT = import.meta.env.VITE_PROJECT || 'projects/demo'
const TOKEN = import.meta.env.VITE_GATEWAY_TOKEN || ''

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API}${path}`, { ...init, headers: { 'content-type': 'application/json', ...(TOKEN ? { authorization: `Bearer ${TOKEN}` } : {}), ...(init?.headers || {}) } })
  if (!response.ok) throw new Error((await response.text()) || `${response.status}`)
  return response.json()
}

function shortName(name: string) { return name.split('/').pop() || name }
function decodeBytes(bytes: number[]) { return new TextDecoder().decode(new Uint8Array(bytes)) }

export default function App() {
  const [topics, setTopics] = useState<Topic[]>([])
  const [subscriptions, setSubscriptions] = useState<Subscription[]>([])
  const [selectedTopic, setSelectedTopic] = useState('')
  const [selectedSub, setSelectedSub] = useState('')
  const [payload, setPayload] = useState('{"order_id":"ORD-1001","status":"created"}')
  const [deliveries, setDeliveries] = useState<Delivery[]>([])
  const [topicName, setTopicName] = useState('orders')
  const [subName, setSubName] = useState('orders-worker')
  const [status, setStatus] = useState('Ready')
  const [busy, setBusy] = useState(false)

  const refresh = useCallback(async () => {
    try {
      const [t, s] = await Promise.all([
        request<Topic[]>(`/admin/v1/topics?project=${encodeURIComponent(PROJECT)}`),
        request<Subscription[]>(`/admin/v1/subscriptions?project=${encodeURIComponent(PROJECT)}`),
      ])
      setTopics(t); setSubscriptions(s)
      if (!selectedTopic && t[0]) setSelectedTopic(t[0].name)
      if (!selectedSub && s[0]) setSelectedSub(s[0].name)
      setStatus('Connected to Relay gateway')
    } catch (e) { setStatus(`Gateway unavailable · ${String(e)}`) }
  }, [selectedSub, selectedTopic])

  useEffect(() => { refresh() }, [refresh])

  const createTopic = async () => {
    if (!topicName.trim()) return
    setBusy(true)
    try {
      const name = `${PROJECT}/topics/${topicName.trim()}`
      await request(`/v1/${name}`, { method: 'PUT', body: JSON.stringify({ labels: { managedBy: 'relay-pubsub-ui' } }) })
      setSelectedTopic(name); setStatus(`Created ${shortName(name)}`); await refresh()
    } catch (e) { setStatus(String(e)) } finally { setBusy(false) }
  }

  const createSubscription = async () => {
    if (!subName.trim() || !selectedTopic) return
    setBusy(true)
    try {
      const name = `${PROJECT}/subscriptions/${subName.trim()}`
      await request(`/v1/${name}`, { method: 'PUT', body: JSON.stringify({ topic: selectedTopic, ackDeadlineSeconds: 20, enableMessageOrdering: true }) })
      setSelectedSub(name); setStatus(`Created ${shortName(name)}`); await refresh()
    } catch (e) { setStatus(String(e)) } finally { setBusy(false) }
  }

  const publish = async () => {
    if (!selectedTopic) return
    setBusy(true)
    try {
      const result = await request<{message_ids:string[]}>('/admin/v1/publish', { method: 'POST', body: JSON.stringify({ topic: selectedTopic, data: payload, attributes: { source: 'relay-console' }, ordering_key: 'demo' }) })
      setStatus(`Published ${result.message_ids[0]}`)
    } catch (e) { setStatus(String(e)) } finally { setBusy(false) }
  }

  const pull = async () => {
    if (!selectedSub) return
    setBusy(true)
    try {
      const result = await request<Delivery[]>('/admin/v1/pull', { method: 'POST', body: JSON.stringify({ subscription: selectedSub, max_messages: 20 }) })
      setDeliveries(result); setStatus(`Pulled ${result.length} message${result.length === 1 ? '' : 's'}`)
    } catch (e) { setStatus(String(e)) } finally { setBusy(false) }
  }

  const ackAll = async () => {
    if (!selectedSub || deliveries.length === 0) return
    setBusy(true)
    try {
      await request('/admin/v1/ack', { method: 'POST', body: JSON.stringify({ subscription: selectedSub, ack_ids: deliveries.map(d => d.ack_id) }) })
      setDeliveries([]); setStatus('Acknowledged all pulled messages')
    } catch (e) { setStatus(String(e)) } finally { setBusy(false) }
  }

  const selectedTopicSubscriptions = useMemo(() => subscriptions.filter(s => s.topic === selectedTopic), [subscriptions, selectedTopic])

  return <div className="shell">
    <aside className="sidebar">
      <div className="brand"><div className="mark">Z</div><div><strong>Zyvor Relay</strong><span>Pub/Sub Gateway</span></div></div>
      <nav>
        <button className="nav active"><Activity size={17}/>Overview</button>
        <button className="nav"><RadioTower size={17}/>Topics <em>{topics.length}</em></button>
        <button className="nav"><DatabaseZap size={17}/>Subscriptions <em>{subscriptions.length}</em></button>
        <button className="nav"><ServerCog size={17}/>Relay Backend</button>
      </nav>
      <div className="side-status"><CircleDot size={14}/><div><b>Gateway</b><small>{status}</small></div></div>
    </aside>

    <main>
      <header><div><p className="eyebrow">EVENT FABRIC / COMPATIBILITY</p><h1>Google Pub/Sub surface.<br/><span>Zyvor Relay underneath.</span></h1></div><button className="ghost" onClick={refresh}><RefreshCw size={16}/>Refresh</button></header>

      <section className="stats">
        <article><span>Topics</span><strong>{topics.length}</strong><small>Google-compatible resources</small></article>
        <article><span>Subscriptions</span><strong>{subscriptions.length}</strong><small>{selectedTopicSubscriptions.length} on selected topic</small></article>
        <article><span>Transports</span><strong>2</strong><small>gRPC :50051 · REST :8080</small></article>
        <article><span>Backend</span><strong>Relay</strong><small>Memory demo or native HTTP</small></article>
      </section>

      <div className="grid two">
        <section className="panel">
          <div className="panel-title"><div><p className="eyebrow">CONTROL PLANE</p><h2>Create resources</h2></div><Plus size={20}/></div>
          <label>Topic name</label>
          <div className="inline"><input value={topicName} onChange={e=>setTopicName(e.target.value)} placeholder="orders"/><button onClick={createTopic} disabled={busy}>Create topic</button></div>
          <label>Subscription topic</label>
          <select value={selectedTopic} onChange={e=>setSelectedTopic(e.target.value)}><option value="">Select topic</option>{topics.map(t=><option key={t.name} value={t.name}>{shortName(t.name)}</option>)}</select>
          <label>Subscription name</label>
          <div className="inline"><input value={subName} onChange={e=>setSubName(e.target.value)} placeholder="orders-worker"/><button onClick={createSubscription} disabled={busy || !selectedTopic}>Create sub</button></div>
        </section>

        <section className="panel accent-panel">
          <div className="panel-title"><div><p className="eyebrow">DATA PLANE</p><h2>Publish event</h2></div><ArrowUpFromLine size={20}/></div>
          <label>Topic</label>
          <select value={selectedTopic} onChange={e=>setSelectedTopic(e.target.value)}><option value="">Select topic</option>{topics.map(t=><option key={t.name} value={t.name}>{t.name}</option>)}</select>
          <label>Payload</label>
          <textarea rows={6} value={payload} onChange={e=>setPayload(e.target.value)}/>
          <button className="primary wide" onClick={publish} disabled={busy || !selectedTopic}><Send size={16}/>Publish through Relay</button>
        </section>
      </div>

      <section className="panel messages">
        <div className="panel-title"><div><p className="eyebrow">CONSUMER</p><h2>Pull & acknowledge</h2></div><ArrowDownToLine size={20}/></div>
        <div className="consumer-bar">
          <select value={selectedSub} onChange={e=>setSelectedSub(e.target.value)}><option value="">Select subscription</option>{subscriptions.map(s=><option key={s.name} value={s.name}>{s.name}</option>)}</select>
          <button onClick={pull} disabled={busy || !selectedSub}>Pull messages</button>
          <button className="success" onClick={ackAll} disabled={busy || deliveries.length===0}><CheckCircle2 size={16}/>Ack all</button>
        </div>
        <div className="message-list">
          {deliveries.length === 0 ? <div className="empty">No pulled messages. Publish an event and pull the subscription.</div> : deliveries.map(d=><article key={d.ack_id} className="message">
            <div><span className="badge">attempt {d.delivery_attempt}</span><b>{d.message.id}</b><small>{new Date(d.message.published_at).toLocaleString()}</small></div>
            <pre>{decodeBytes(d.message.data)}</pre>
            <code>{d.ack_id}</code>
          </article>)}
        </div>
      </section>
    </main>
  </div>
}
