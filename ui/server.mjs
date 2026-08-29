#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0
//
// Serves the product UI over HTTPS and proxies gateway REST paths to
// GATEWAY_UPSTREAM (insecure TLS OK). Same-origin fetch avoids the browser
// rejecting a second self-signed cert on :8081.

import fs from 'node:fs'
import https from 'node:https'
import http from 'node:http'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const DIST = process.env.CONSOLE_DIST || path.join(__dirname, 'dist')
const PORT = Number(process.env.CONSOLE_PORT || 8082)
const HOST = process.env.CONSOLE_HOST || '0.0.0.0'
const CERT = process.env.CONSOLE_TLS_CERT || '/var/lib/relay-pubsub-console/tls/cert.pem'
const KEY = process.env.CONSOLE_TLS_KEY || '/var/lib/relay-pubsub-console/tls/key.pem'
const SAN = process.env.CONSOLE_TLS_SAN || 'localhost,relay-pubsub-console'
const UPSTREAM = (process.env.GATEWAY_UPSTREAM || 'https://127.0.0.1:8081').replace(/\/$/, '')

const PROXY_PREFIXES = ['/v1', '/admin', '/healthz', '/readyz', '/metrics']

function isProxyPath(urlPath) {
  return PROXY_PREFIXES.some((p) => urlPath === p || urlPath.startsWith(p + '/') || urlPath.startsWith(p + '?'))
}

function contentType(filePath) {
  const ext = path.extname(filePath).toLowerCase()
  return (
    {
      '.html': 'text/html; charset=utf-8',
      '.js': 'application/javascript; charset=utf-8',
      '.css': 'text/css; charset=utf-8',
      '.json': 'application/json',
      '.svg': 'image/svg+xml',
      '.png': 'image/png',
      '.ico': 'image/x-icon',
      '.woff2': 'font/woff2',
    }[ext] || 'application/octet-stream'
  )
}

function ensureTls() {
  if (fs.existsSync(CERT) && fs.existsSync(KEY)) return
  const dir = path.dirname(CERT)
  fs.mkdirSync(dir, { recursive: true })
  const names = SAN.split(',')
    .map((s) => s.trim())
    .filter(Boolean)
  const alt = names
    .map((n, i) => (/^\d+\.\d+\.\d+\.\d+$/.test(n) ? `IP.${i + 1}=${n}` : `DNS.${i + 1}=${n}`))
    .join('\n')
  const conf = path.join(dir, 'openssl.cnf')
  fs.writeFileSync(
    conf,
    `[req]
default_bits = 2048
prompt = no
default_md = sha256
distinguished_name = dn
x509_extensions = v3
[dn]
CN = ${names[0] || 'localhost'}
[v3]
subjectAltName = @alt
basicConstraints = CA:FALSE
[alt]
${alt}
`,
  )
  execFileSync(
    'openssl',
    [
      'req',
      '-x509',
      '-newkey',
      'rsa:2048',
      '-nodes',
      '-keyout',
      KEY,
      '-out',
      CERT,
      '-days',
      '825',
      '-config',
      conf,
      '-extensions',
      'v3',
    ],
    { stdio: 'inherit' },
  )
  console.log(`generated self-signed TLS at ${CERT}`)
}

function serveStatic(req, res) {
  let urlPath = decodeURIComponent((req.url || '/').split('?')[0])
  if (urlPath === '/') urlPath = '/index.html'
  const filePath = path.normalize(path.join(DIST, urlPath))
  if (!filePath.startsWith(DIST)) {
    res.writeHead(403)
    res.end('forbidden')
    return
  }
  fs.readFile(filePath, (err, data) => {
    if (err) {
      fs.readFile(path.join(DIST, 'index.html'), (err2, html) => {
        if (err2) {
          res.writeHead(404)
          res.end('not found')
          return
        }
        res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
        res.end(html)
      })
      return
    }
    res.writeHead(200, { 'content-type': contentType(filePath) })
    res.end(data)
  })
}

function proxy(req, res) {
  const target = new URL(req.url || '/', UPSTREAM)
  const lib = target.protocol === 'https:' ? https : http
  const headers = { ...req.headers, host: target.host }
  delete headers['connection']

  const upstream = lib.request(
    {
      protocol: target.protocol,
      hostname: target.hostname,
      port: target.port || (target.protocol === 'https:' ? 443 : 80),
      path: target.pathname + target.search,
      method: req.method,
      headers,
      rejectUnauthorized: false,
    },
    (upRes) => {
      const outHeaders = { ...upRes.headers }
      delete outHeaders['access-control-allow-origin']
      delete outHeaders['access-control-allow-methods']
      delete outHeaders['access-control-allow-headers']
      res.writeHead(upRes.statusCode || 502, outHeaders)
      upRes.pipe(res)
    },
  )
  upstream.on('error', (e) => {
    res.writeHead(502, { 'content-type': 'application/json' })
    res.end(JSON.stringify({ error: { message: `gateway proxy: ${e.message}` } }))
  })
  req.pipe(upstream)
}

function handler(req, res) {
  const urlPath = (req.url || '/').split('?')[0]
  if (isProxyPath(urlPath)) {
    proxy(req, res)
    return
  }
  serveStatic(req, res)
}

ensureTls()

const server = https.createServer(
  {
    cert: fs.readFileSync(CERT),
    key: fs.readFileSync(KEY),
  },
  handler,
)

server.listen(PORT, HOST, () => {
  console.log(`relay-pubsub-console listening https://${HOST}:${PORT}`)
  console.log(`  static: ${DIST}`)
  console.log(`  proxy → ${UPSTREAM}`)
})
