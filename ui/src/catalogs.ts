// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

/** Mirrors src/config.rs catalogs — topic short names = Relay event types. */

export const FASAL_CATALOG = [
  'irrigation.required',
  'soil.moisture.critical',
  'fertigation.required',
  'disease.risk.critical',
  'device.control.required',
  'crop.advisory',
  'weather.advisory',
  'spray.advisory',
  'frost.alert',
  'pest.advisory',
] as const

export const EDGE_CATALOG = [
  'firewater.tank.low',
  'firewater.pressure.low',
  'firewater.demand.active',
  'firewater.pump.fail',
  'firewater.valve.closed',
  'firewater.flow.detected',
  'firewater.freeze.risk',
  'firewater.hydrant.tamper',
  'firewater.leak.acoustic',
  'firewater.pump.vibration',
  'edge.vision.fire',
  'edge.comms.down',
  'edge.power.fail',
  'edge.gas.alarm',
  'edge.control.fault',
  'edge.access.breach',
  'edge.runtime.down',
  'telemetry.sample',
] as const

export const REMOTE_EDGE_CATALOG = [
  'remote-edge.link.starlink.degraded',
  'remote-edge.link.offline',
  'remote-edge.galleon.thermal',
  'remote-edge.vision.intrusion',
  'remote-edge.iot.flood',
  'remote-edge.uav.rtb',
] as const

export const FLEET_CATALOG = [
  'fleet.power.island',
  'fleet.robot.lost',
  'fleet.ot.ids',
  'fleet.env.exceedance',
  'fleet.dc.thermal',
  'fleet.access.fault',
] as const

export type CatalogId = 'fasal' | 'edge' | 'remote-edge' | 'fleet' | 'actions' | 'demo' | 'tests'

export type CatalogDef = {
  id: CatalogId
  title: string
  description: string
  /** Short topic names (no projects/… prefix). */
  topics: readonly string[]
  /** If true, also create `<topic>-sub` for each topic. */
  withSubs: boolean
  /** Extra full subscription specs (name relative to project). */
  extraSubs?: { name: string; topic: string }[]
}

export const CATALOGS: CatalogDef[] = [
  {
    id: 'fasal',
    title: 'Farm (Fasal)',
    description: '10 farm event types',
    topics: FASAL_CATALOG,
    withSubs: true,
  },
  {
    id: 'edge',
    title: 'Edge / firewater',
    description: '18 industrial + edge types',
    topics: EDGE_CATALOG,
    withSubs: true,
  },
  {
    id: 'remote-edge',
    title: 'Remote edge',
    description: '6 remote-edge simulator types',
    topics: REMOTE_EDGE_CATALOG,
    withSubs: true,
  },
  {
    id: 'fleet',
    title: 'Fleet',
    description: '6 fleet-wide types',
    topics: FLEET_CATALOG,
    withSubs: true,
  },
  {
    id: 'actions',
    title: 'Actions queue',
    description: 'Relay → pull outbound acts',
    topics: ['farm-actions'],
    withSubs: false,
    extraSubs: [{ name: 'farm-actions-sub', topic: 'farm-actions' }],
  },
  {
    id: 'demo',
    title: 'Live demo',
    description: 'Resources used by #demo',
    topics: ['demo.hello'],
    withSubs: false,
    extraSubs: [{ name: 'demo.hello-sub', topic: 'demo.hello' }],
  },
  {
    id: 'tests',
    title: 'Test suite',
    description: 'Smoke + conformance resources',
    topics: ['orders', 'conformance-ui', 'page-ui-1', 'page-ui-2', 'page-ui-3'],
    withSubs: false,
    extraSubs: [
      { name: 'orders-worker', topic: 'orders' },
      { name: 'conformance-ui-sub', topic: 'conformance-ui' },
    ],
  },
]

export function allCatalogTopics(): string[] {
  const set = new Set<string>()
  for (const c of CATALOGS) {
    for (const t of c.topics) set.add(t)
  }
  return [...set].sort()
}
