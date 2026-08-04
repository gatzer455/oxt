---
titulo: Contrato de salida y errores JSON
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: [forma-plana-de-la-cli]
---

## Pregunta

¿Cuál es la forma exacta del JSON de cada comando en stdout (`--json`), del error en stderr, y la tabla de exit codes tipificados?

Ya decidido al trazar: flag global `--json` (texto por defecto), errores JSON en stderr, exit codes tipificados, `oxt schema` aparte (ticket propio).

Ramas a cerrar:

- Envoltorio de éxito: ¿cada comando emite su payload directo (`{...}`) o una envoltura (`{"ok": true, "result": ...}`)? Alimentar con los hallazgos de «Convenciones de CLIs harness-first».
- Campos por comando: `read` (¿reusa `ReadOutput` de `agent.rs` o cambia?), `edit` (hoy `EditResult { replacements, affected_parts }`), `create` (¿path creado? ¿id/url si es Google?), `info` (hoy JSON ad-hoc), `auth`/`list`/`download`.
- Errores: forma exacta del envelope (`{"error": {"code": ..., "message": ...}}`), campos adicionales (¿`detail`/`hint`?), y qué va en stdout vs stderr.
- Exit codes: tabla estable y documentada — 0 ok, 2 argumentos (clap), y los códigos de dominio (¿3 archivo/IO, 4 IR inválido, 5 auth/Google, 6 formato no soportado?). Mapear los errores internos (`DocxError`, `GoogleError`, `Error` de `lib.rs`) a códigos.
- Cómo se aplica `--json` a los errores: ¿el error es JSON solo si `--json` está presente, o siempre JSON en stderr?

Criterio de salida: un harness puede escribir parsers para todos los comandos y ramas de error solo con este contrato, sin leer código.

## Resolución (2026-07-30, ráfaga autorizada — provisional)

Contrato completo en `docs/cli.md` §Errores, §Outcomes, §Salida JSON por comando:

- Envelope de error: `{"kind", "message", "hint"?}` — última línea de stderr, siempre JSON, incluso sin `--json`. Clap via `try_parse` → envelope `usage`.
- Exit codes: 0 ok; 2 usage; 3 io_error; 4 unsupported_format/parse_error; 5 invalid_ir; 6 auth_error; 7 api_error; 8 edit_error; 10 internal_error.
- Outcomes sin envelope: grep 1 (sin matches), diff 1 (hay diferencias), auth status 1 (no autenticado). `internal_error` en 10 para no pisar el 1.
- Envelope de éxito: payload directo por comando, sin wrapper genérico; `read` con `{"format", "data"}`; mutantes con `changed` (edit, update).
- Claves en inglés, mensajes en español; stdout solo datos.

Fuentes: hallazgos de «Convenciones de CLIs harness-first».
