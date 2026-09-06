// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

import { useCallback, useEffect, useState } from 'react'
import { fetchInventory, request } from './api/gateway'

const STORAGE_KEY = 'relay_pubsub_onboarded'

const STEPS = [
  {
    id: 'cert',
    title: 'Open console',
    desc: 'Accept the self-signed cert if prompted, then keep this tab open.',
    href: '#top',
  },
  {
    id: 'connected',
    title: 'Gateway connected',
    desc: 'Same-origin API must answer /healthz (proxied to the gateway).',
    href: '#console',
  },
  {
    id: 'generate',
    title: 'Generate catalogs',
    desc: 'Seed Demo or Farm topics into memory.',
    href: '#generate',
  },
  {
    id: 'demo',
    title: 'Live demo',
    desc: 'Publish → pull → ack against the real gateway.',
    href: '#demo',
  },
  {
    id: 'stored',
    title: 'Stored console',
    desc: 'Confirm inventory under Console → Stored.',
    href: '#console',
  },
] as const

type StepId = (typeof STEPS)[number]['id']

function readDismissed(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

export function reopenOnboarding() {
  try {
    localStorage.removeItem(STORAGE_KEY)
  } catch {
    /* ignore */
  }
  window.dispatchEvent(new Event('relay-pubsub-onboard-open'))
}

type Props = {
  /** Optional override when parent already knows inventory emptiness. */
  inventoryEmpty?: boolean
}

export function Onboarding({ inventoryEmpty: inventoryEmptyProp }: Props) {
  const [open, setOpen] = useState(() => !readDismissed())
  const [connected, setConnected] = useState(false)
  const [inventoryEmpty, setInventoryEmpty] = useState(inventoryEmptyProp ?? true)
  const [current, setCurrent] = useState<StepId>('cert')

  const dismiss = useCallback(() => {
    try {
      localStorage.setItem(STORAGE_KEY, '1')
    } catch {
      /* ignore */
    }
    setOpen(false)
  }, [])

  useEffect(() => {
    const onOpen = () => setOpen(true)
    window.addEventListener('relay-pubsub-onboard-open', onOpen)
    return () => window.removeEventListener('relay-pubsub-onboard-open', onOpen)
  }, [])

  useEffect(() => {
    if (!open) return
    let cancelled = false
    ;(async () => {
      try {
        await request('/healthz')
        if (!cancelled) {
          setConnected(true)
          setCurrent((c) => (c === 'cert' || c === 'connected' ? 'generate' : c))
        }
      } catch {
        if (!cancelled) {
          setConnected(false)
          setCurrent('connected')
        }
      }
      if (inventoryEmptyProp !== undefined) {
        if (!cancelled) setInventoryEmpty(inventoryEmptyProp)
        return
      }
      try {
        const inv = await fetchInventory()
        if (!cancelled) {
          const empty = inv.topics.length === 0
          setInventoryEmpty(empty)
          if (!empty) setCurrent((c) => (c === 'generate' ? 'demo' : c))
        }
      } catch {
        /* keep previous */
      }
    })()
    return () => {
      cancelled = true
    }
  }, [open, inventoryEmptyProp])

  useEffect(() => {
    if (inventoryEmptyProp !== undefined) setInventoryEmpty(inventoryEmptyProp)
  }, [inventoryEmptyProp])

  if (!open) return null

  const done = (id: StepId) => {
    if (id === 'cert') return true
    if (id === 'connected') return connected
    if (id === 'generate') return inventoryEmpty === false
    return false
  }

  return (
    <aside className="onboard" role="dialog" aria-labelledby="onboardTitle">
      <p className="onboard-kicker">First install</p>
      <h3 id="onboardTitle">Get started</h3>
      <p className="onboard-lead">
        Generate catalogs, run the live demo, then open Stored. Relay JWT stays on the gateway host
        env — not in this browser. Peers may be remote; the console proxies to whichever upstream
        was set at deploy time.
      </p>
      <ol className="onboard-steps">
        {STEPS.map((s, i) => {
          const isDone = done(s.id)
          const isCurrent = current === s.id && !isDone
          return (
            <li key={s.id} className={isDone ? 'done' : isCurrent ? 'current' : undefined}>
              <span className="onboard-num">{i + 1}</span>
              <div>
                <div className="onboard-step-title">{s.title}</div>
                <p className="onboard-step-desc">{s.desc}</p>
              </div>
              <span className="onboard-mark">{isDone ? 'done' : '—'}</span>
            </li>
          )
        })}
      </ol>
      <div className="onboard-actions">
        <a
          className="btn btn-primary"
          href={STEPS.find((s) => s.id === current)?.href || '#generate'}
          onClick={() => {
            const idx = STEPS.findIndex((s) => s.id === current)
            const next = STEPS[Math.min(idx + 1, STEPS.length - 1)]
            setCurrent(next.id)
          }}
        >
          Continue
        </a>
        <button type="button" className="btn btn-ghost" onClick={dismiss}>
          Skip
        </button>
        <button type="button" className="btn btn-dark" onClick={dismiss}>
          Done
        </button>
      </div>
      <p className="onboard-note">Reopen anytime from Console → Stored empty state or Configure.</p>
    </aside>
  )
}
