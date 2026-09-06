// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

export const API = import.meta.env.VITE_API_BASE ?? ''
export const PROJECT = import.meta.env.VITE_PROJECT || 'projects/demo'
export const TOKEN = import.meta.env.VITE_GATEWAY_TOKEN || ''

export type Topic = {
  name: string
  labels: Record<string, string>
  kms_key_name?: string
}

export type Subscription = {
  name: string
  topic: string
  ack_deadline_seconds: number
  enable_message_ordering: boolean
  enable_exactly_once_delivery: boolean
  push_endpoint?: string | null
  push_attributes?: Record<string, string>
}

export type Delivery = {
  ack_id: string
  delivery_attempt: number
  message: {
    id: string
    data: number[]
    attributes: Record<string, string>
    ordering_key: string
    published_at: string
  }
}

export type MessagePreview = {
  id: string
  data: string
  attributes: Record<string, string>
  ordering_key: string
  published_at: string
}

export type TopicInventory = {
  name: string
  labels: Record<string, string>
  message_count: number
  recent: MessagePreview[]
}

export type SubscriptionInventory = {
  name: string
  topic: string
  ack_deadline_seconds: number
  enable_message_ordering: boolean
  enable_exactly_once_delivery: boolean
  push_endpoint?: string | null
  push_attributes?: Record<string, string>
  dead_letter_topic?: string | null
  topic_message_count: number
  next_index: number
  backlog: number
  inflight: number
  retry_queued: number
  acked: number
}

export type InventoryReport = {
  topics: TopicInventory[]
  subscriptions: SubscriptionInventory[]
}

export type LogEntry = {
  id: number
  ts: string
  level: string
  target: string
  message: string
}

export type LogsResponse = {
  entries: LogEntry[]
  next_after: number
}

export async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const base = API.replace(/\/$/, '')
  const response = await fetch(`${base}${path}`, {
    ...init,
    headers: {
      'content-type': 'application/json',
      ...(TOKEN ? { authorization: `Bearer ${TOKEN}` } : {}),
      ...(init?.headers || {}),
    },
  })
  if (!response.ok) throw new Error((await response.text()) || `${response.status}`)
  const text = await response.text()
  if (!text) return {} as T
  return JSON.parse(text) as T
}

export function shortName(name: string) {
  return name.split('/').pop() || name
}

export function decodeBytes(bytes: number[]) {
  return new TextDecoder().decode(new Uint8Array(bytes))
}

export function toBase64(str: string) {
  const bytes = new TextEncoder().encode(str)
  let binary = ''
  bytes.forEach((b) => {
    binary += String.fromCharCode(b)
  })
  return btoa(binary)
}

export async function ensureTopic(name: string, labels: Record<string, string> = {}) {
  try {
    await request(`/v1/${name}`, { method: 'PUT', body: JSON.stringify({ labels }) })
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('ALREADY_EXISTS') && !msg.includes('409') && !msg.includes('already exists')) {
      try {
        await request(`/v1/${name}`)
      } catch {
        throw e
      }
    }
  }
}

export async function ensureSubscription(
  name: string,
  topic: string,
  extras: Record<string, unknown> = {},
) {
  try {
    await request(`/v1/${name}`, {
      method: 'PUT',
      body: JSON.stringify({
        topic,
        ackDeadlineSeconds: 20,
        enableMessageOrdering: true,
        ...extras,
      }),
    })
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('ALREADY_EXISTS') && !msg.includes('409') && !msg.includes('already exists')) {
      try {
        await request(`/v1/${name}`)
      } catch {
        throw e
      }
    }
  }
}

export async function fetchInventory(): Promise<InventoryReport> {
  return request<InventoryReport>(`/admin/v1/inventory?project=${encodeURIComponent(PROJECT)}`)
}

export async function fetchLogs(after = 0, limit = 200): Promise<LogsResponse> {
  const q = new URLSearchParams()
  if (after > 0) q.set('after', String(after))
  q.set('limit', String(limit))
  return request<LogsResponse>(`/admin/v1/logs?${q}`)
}
