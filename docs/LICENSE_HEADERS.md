---
hero:
  eyebrow: LICENSE HEADERS
  title: Source license headers
---

All **source files** in this repository carry SPDX metadata (`LicenseRef-Zyvor-Production-1.0`, Zyvor Production License v1.0) at the top of the file.

← [Docs hub](README.md)

---

## Required header

```text
Copyright 2026 Zyvor AI Labs · https://zyvor.dev
SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
```

| File type | Prefix |
|-----------|--------|
| `.rs` | `//` |
| `.sh`, `.py`, `Dockerfile` | `#` |
| `.html` | `<!-- … -->` |
| `.ts`, `.tsx`, `.js` | `//` |

Shell scripts: shebang line first, then copyright/SPDX lines.

---

## Scope

| Included | Excluded |
|----------|----------|
| `src/*.rs`, `build.rs`, scripts, `ui/src/*` | Markdown docs |
| Ops console under `ui/` | `target/`, `node_modules/` |

Full license: [LICENSE](https://github.com/zyvorai/relay-pubsub/blob/main/LICENSE).

---

## Related

- [relay-edge docs](https://github.com/zyvorai/relay-edge/blob/main/docs/LICENSE_HEADERS.md)
- relay docs
