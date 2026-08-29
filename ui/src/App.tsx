// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

import { Console } from './Console'
import { LiveDemo } from './demo/LiveDemo'
import { TestSuite } from './demo/TestSuite'
import { Generate } from './Generate'
import { HeroFlow } from './HeroFlow'
import { ZyvorLogo } from './ZyvorLogo'

export default function App() {
  return (
    <>
      <header className="nav-bar">
        <div className="nav-brand">
          <ZyvorLogo size={28} />
          <span className="nav-brand-divider" aria-hidden />
          <a className="nav-brand-product" href="#top">
            Relay Pub/Sub
          </a>
        </div>
        <nav className="nav-links">
          <a href="#how">How it works</a>
          <a href="#generate">Generate</a>
          <a href="#demo">Demo</a>
          <a href="#tests">Tests</a>
          <a href="#console">Console</a>
        </nav>
        <a className="nav-cta" href="#generate">
          Generate
        </a>
      </header>

      <section className="hero" id="top">
        <div className="hero-eyebrow">
          <ZyvorLogo size={22} wordmark href="https://zyvor.dev" />
          <a className="hero-eyebrow-link" href="https://zyvor.dev" target="_blank" rel="noopener noreferrer">
            zyvor.dev
          </a>
        </div>
        <h1 className="hero-brand">
          Relay <em>Pub/Sub</em>
        </h1>
        <p className="hero-sub">
          Speak Google Cloud Pub/Sub. Deliver through Zyvor Relay — TLS, catalogs, and a live path from publish to act.
        </p>
        <div className="hero-ctas">
          <a className="btn btn-primary" href="#generate">
            Generate catalogs
          </a>
          <a className="btn btn-ghost" href="#demo">
            Try live demo →
          </a>
        </div>
        <HeroFlow />
      </section>

      <section className="section" id="how">
        <p className="section-kicker">How it works</p>
        <h2 className="section-title">Familiar APIs. Relay underneath.</h2>
        <p className="section-lead">
          Publishers and subscribers keep their SDKs. The gateway translates topics into Relay events and actions back into pull queues.
        </p>
        <div className="story-grid">
          <div className="story-item">
            <h3>Publish</h3>
            <p>gRPC or REST publish lands as <code>POST /v1/events</code> when you use the relay-events backend.</p>
          </div>
          <div className="story-item">
            <h3>Act</h3>
            <p>Relay calls the Action Gateway; outbound work queues on a local subscription you can pull.</p>
          </div>
          <div className="story-item">
            <h3>Prove</h3>
            <p>Generate catalogs into memory, then run the in-page suite against the same resources.</p>
          </div>
        </div>
      </section>

      <section className="section full-bleed" id="generate">
        <div className="section-inner">
          <p className="section-kicker">Generate</p>
          <h2 className="section-title">Seed memory from catalogs.</h2>
          <p className="section-lead">
            Create farm, edge, fleet, demo, and test topics/subscriptions in the gateway — then see exactly what is already there.
          </p>
          <Generate />
        </div>
      </section>

      <section className="section" id="demo">
        <p className="section-kicker">Live demo</p>
        <h2 className="section-title">Publish. Pull. Ack.</h2>
        <p className="section-lead">
          One click against the real gateway. Prefer generating the Demo catalog first if the topic is missing.
        </p>
        <LiveDemo />
      </section>

      <section className="section full-bleed" id="tests">
        <div className="section-inner">
          <p className="section-kicker">Test suite</p>
          <h2 className="section-title">Health, smoke, conformance.</h2>
          <p className="section-lead">
            Browser-side runners for the HTTP surface. Generate the Test suite catalog first for a clean run.
          </p>
          <TestSuite />
        </div>
      </section>

      <section className="section" id="console">
        <p className="section-kicker">Ops console</p>
        <h2 className="section-title">Incoming. Outgoing. Stored.</h2>
        <p className="section-lead">
          Configure topics and push endpoints, publish inbound payloads, pull or push outbound
          deliveries, peek stored messages, and tail live gateway logs.
        </p>
        <Console />
      </section>

      <footer className="footer">
        <div className="footer-brand">
          <ZyvorLogo size={24} />
        </div>
        <p className="footer-copy">
          Copyright © {new Date().getFullYear()}{' '}
          <a href="https://zyvor.dev" target="_blank" rel="noopener noreferrer">
            Zyvor
          </a>
          . All rights reserved.
        </p>
        <p className="footer-meta">
          <a href="https://zyvor.dev" target="_blank" rel="noopener noreferrer">
            zyvor.dev
          </a>
          {' · '}
          Relay Pub/Sub · Apache-2.0 · Zyvor AI Labs
        </p>
      </footer>
    </>
  )
}
