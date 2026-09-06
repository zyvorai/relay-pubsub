#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

// REST publish/pull/ack against a TLS gateway (self-signed OK).

import https from 'node:https'
import { URL } from 'node:url'

const BASE = (process.env.BASE || 'https://127.0.0.1:8080').replace(/\/$/, '')
const PROJECT = process.env.PROJECT || 'projects/demo'
const agent = new https.Agent({ rejectUnauthorized: false })

function req(method, path, body) {
  const url = new URL(path, BASE)
  const payload = body ? JSON.stringify(body) : null
  return new Promise((resolve, reject) => {
    const r = https.request(
      url,
      {
        method,
        headers: { 'content-type': 'application/json', ...(payload ? { 'content-length': Buffer.byteLength(payload) } : {}) },
        agent,
      },
      (resp) => {
        const chunks = []
        resp.on('data', (c) => chunks.push(c))
        resp.on('end', () => {
          const text = Buffer.concat(chunks).toString()
          const status = resp.statusCode || 0
          if (status >= 300 && status !== 409) {
            reject(new Error(`${method} ${path} → ${status} ${text}`))
            return
          }
          resolve(text ? JSON.parse(text) : {})
        })
      },
    )
    r.on('error', reject)
    if (payload) r.write(payload)
    r.end()
  })
}

const topic = `${PROJECT}/topics/matrix-node`
const sub = `${PROJECT}/subscriptions/matrix-node-sub`

await req('PUT', `/v1/${topic}`, { labels: { lane: 'node' } }).catch(() => ({}))
await req('PUT', `/v1/${sub}`, { topic, ackDeadlineSeconds: 20 }).catch(() => ({}))
const pub = await req('POST', `/v1/${topic}:publish`, {
  messages: [{ data: Buffer.from('node').toString('base64'), attributes: { source: 'matrix-node' } }],
})
if (!pub.messageIds?.length) throw new Error(`publish failed: ${JSON.stringify(pub)}`)
const pulled = await req('POST', `/v1/${sub}:pull`, { maxMessages: 5 })
const msgs = pulled.receivedMessages || []
if (!msgs.length) throw new Error('no messages')
await req('POST', `/v1/${sub}:acknowledge`, { ackIds: msgs.map((m) => m.ackId) })
console.log('node-rest ok', pub.messageIds[0])
