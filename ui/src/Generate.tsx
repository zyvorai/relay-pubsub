// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  PROJECT,
  ensureSubscription,
  ensureTopic,
  request,
  shortName,
  type Subscription,
  type Topic,
} from './api/gateway'
import { CATALOGS, type CatalogId } from './catalogs'

type Inventory = {
  topics: Topic[]
  subscriptions: Subscription[]
}

function topicFull(short: string) {
  return `${PROJECT}/topics/${short}`
}

function subFull(short: string) {
  return `${PROJECT}/subscriptions/${short}`
}

export function Generate({ onChanged }: { onChanged?: () => void }) {
  const [inv, setInv] = useState<Inventory>({ topics: [], subscriptions: [] })
  const [busy, setBusy] = useState<CatalogId | 'all' | 'refresh' | null>(null)
  const [msg, setMsg] = useState('Select a catalog to create topics and subscriptions in gateway memory.')
  const [err, setErr] = useState('')

  const refresh = useCallback(async () => {
    setBusy('refresh')
    setErr('')
    try {
      const [topics, subscriptions] = await Promise.all([
        request<Topic[]>(`/admin/v1/topics?project=${encodeURIComponent(PROJECT)}`),
        request<Subscription[]>(`/admin/v1/subscriptions?project=${encodeURIComponent(PROJECT)}`),
      ])
      setInv({ topics, subscriptions })
      setMsg(`${topics.length} topics · ${subscriptions.length} subscriptions in memory`)
      onChanged?.()
    } catch (e) {
      setErr(String(e))
    } finally {
      setBusy(null)
    }
  }, [onChanged])

  useEffect(() => {
    refresh()
  }, [refresh])

  const topicSet = useMemo(() => new Set(inv.topics.map((t) => shortName(t.name))), [inv.topics])
  const subSet = useMemo(
    () => new Set(inv.subscriptions.map((s) => shortName(s.name))),
    [inv.subscriptions],
  )

  const catalogStatus = useMemo(() => {
    return CATALOGS.map((c) => {
      const topicPresent = c.topics.filter((t) => topicSet.has(t)).length
      const subNames = [
        ...(c.withSubs ? c.topics.map((t) => `${t}-sub`) : []),
        ...(c.extraSubs?.map((s) => s.name) || []),
      ]
      const subPresent = subNames.filter((s) => subSet.has(s)).length
      return {
        ...c,
        topicPresent,
        topicTotal: c.topics.length,
        subPresent,
        subTotal: subNames.length,
        complete: topicPresent === c.topics.length && subPresent === subNames.length,
      }
    })
  }, [topicSet, subSet])

  const seedCatalog = async (id: CatalogId) => {
    const cat = CATALOGS.find((c) => c.id === id)
    if (!cat) return { createdTopics: 0, createdSubs: 0 }
    let createdTopics = 0
    let createdSubs = 0
    const topicsNow = new Set(inv.topics.map((t) => shortName(t.name)))
    const subsNow = new Set(inv.subscriptions.map((s) => shortName(s.name)))

    for (const short of cat.topics) {
      const before = topicsNow.has(short)
      await ensureTopic(topicFull(short), {
        catalog: cat.id,
        managedBy: 'relay-pubsub-ui',
      })
      if (!before) {
        createdTopics++
        topicsNow.add(short)
      }
      if (cat.withSubs) {
        const subShort = `${short}-sub`
        const had = subsNow.has(subShort)
        await ensureSubscription(subFull(subShort), topicFull(short), {
          enableMessageOrdering: true,
        })
        if (!had) {
          createdSubs++
          subsNow.add(subShort)
        }
      }
    }
    for (const s of cat.extraSubs || []) {
      await ensureTopic(topicFull(s.topic), { catalog: cat.id, managedBy: 'relay-pubsub-ui' })
      topicsNow.add(s.topic)
      const had = subsNow.has(s.name)
      await ensureSubscription(subFull(s.name), topicFull(s.topic), {
        enableMessageOrdering: true,
      })
      if (!had) {
        createdSubs++
        subsNow.add(s.name)
      }
    }
    return { createdTopics, createdSubs }
  }

  const generateOne = async (id: CatalogId) => {
    const cat = CATALOGS.find((c) => c.id === id)
    if (!cat) return
    setBusy(id)
    setErr('')
    try {
      const { createdTopics, createdSubs } = await seedCatalog(id)
      setMsg(
        `Generated ${cat.title}: +${createdTopics} topics, +${createdSubs} subscriptions (existing skipped)`,
      )
      await refresh()
    } catch (e) {
      setErr(String(e))
    } finally {
      setBusy(null)
    }
  }

  const generateAll = async () => {
    setBusy('all')
    setErr('')
    try {
      let topics = 0
      let subs = 0
      for (const c of CATALOGS) {
        const r = await seedCatalog(c.id)
        topics += r.createdTopics
        subs += r.createdSubs
        // Refresh inventory between catalogs so "existing" counts stay accurate.
        const [tList, sList] = await Promise.all([
          request<Topic[]>(`/admin/v1/topics?project=${encodeURIComponent(PROJECT)}`),
          request<Subscription[]>(`/admin/v1/subscriptions?project=${encodeURIComponent(PROJECT)}`),
        ])
        setInv({ topics: tList, subscriptions: sList })
      }
      setMsg(`Generated all: +${topics} topics, +${subs} subscriptions`)
      await refresh()
    } catch (e) {
      setErr(String(e))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="demo-panel generate-panel">
      <div className="suite-actions">
        <button className="btn btn-primary" onClick={generateAll} disabled={busy !== null}>
          {busy === 'all' ? 'Generating…' : 'Generate all catalogs + demo + tests'}
        </button>
        <button className="btn btn-ghost" onClick={refresh} disabled={busy !== null}>
          Refresh inventory
        </button>
      </div>
      <p className={`demo-status ${err ? 'err' : ''}`}>{err || msg}</p>

      <div className="generate-grid">
        {catalogStatus.map((c) => (
          <article key={c.id} className={`generate-card ${c.complete ? 'complete' : ''}`}>
            <header>
              <h3>{c.title}</h3>
              <span className="gen-badge">
                {c.complete ? 'ready' : `${c.topicPresent}/${c.topicTotal} topics`}
              </span>
            </header>
            <p>{c.description}</p>
            <p className="gen-meta">
              Topics {c.topicPresent}/{c.topicTotal}
              {c.subTotal > 0 ? ` · Subs ${c.subPresent}/${c.subTotal}` : ''}
            </p>
            <ul className="gen-list">
              {c.topics.slice(0, 6).map((t) => (
                <li key={t} className={topicSet.has(t) ? 'on' : 'off'}>
                  {topicSet.has(t) ? '●' : '○'} {t}
                </li>
              ))}
              {c.topics.length > 6 && (
                <li className="more">+{c.topics.length - 6} more</li>
              )}
            </ul>
            <button
              className="btn btn-dark"
              style={{ width: '100%', marginTop: 12 }}
              onClick={() => generateOne(c.id)}
              disabled={busy !== null}
            >
              {busy === c.id ? 'Generating…' : c.complete ? 'Re-ensure' : 'Generate'}
            </button>
          </article>
        ))}
      </div>

      <div className="memory-table-wrap">
        <h3 className="memory-heading">In memory now</h3>
        <div className="memory-cols">
          <div>
            <h4>Topics ({inv.topics.length})</h4>
            {inv.topics.length === 0 ? (
              <p className="empty">None — generate a catalog above.</p>
            ) : (
              <ul className="memory-list">
                {inv.topics.map((t) => (
                  <li key={t.name}>{shortName(t.name)}</li>
                ))}
              </ul>
            )}
          </div>
          <div>
            <h4>Subscriptions ({inv.subscriptions.length})</h4>
            {inv.subscriptions.length === 0 ? (
              <p className="empty">None</p>
            ) : (
              <ul className="memory-list">
                {inv.subscriptions.map((s) => (
                  <li key={s.name}>
                    <strong>{shortName(s.name)}</strong>
                    <span> ← {shortName(s.topic)}</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
