# Source license headers

All **source files** in this repository carry Apache-2.0 SPDX metadata at the top of the file.

← [Docs hub](README.md)

---

## Required header

```text
Copyright 2026 Zyvor AI Labs · https://zyvor.dev
SPDX-License-Identifier: Apache-2.0
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

Full license: [LICENSE](../LICENSE).

---

## Related

- [relay-edge docs](https://github.com/zyvorai/relay-edge/blob/main/docs/LICENSE_HEADERS.md)
- [relay docs](https://github.com/zyvorai/relay/blob/main/docs/LICENSE_HEADERS.md)
