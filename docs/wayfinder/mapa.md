# Mapa: CLI de oxt usable por cualquier harness

> Tracker local de markdown — `docs/wayfinder/`. Cada ticket es un archivo en `tickets/` con frontmatter (tipo, estado, asignado, bloqueado-por). Tomar un ticket = escribir `asignado:` en su frontmatter antes de trabajar. Resolver = responder en el ticket + `estado: cerrado` + línea en Decisiones hasta ahora.

## Destino

Una especificación escrita del contrato CLI de oxt —CLI plana unificada local+Google, salida JSON canónica con `--json`, errores JSON en stderr, exit codes tipificados, entrada por stdin (`-`), descubrimiento con `oxt schema`— y la CLI implementada conforme a esa especificación en el repo. Además, **seis funcionalidades nuevas** decididas al trazar: `update` (escribir IR en archivo local existente), `convert` (entre formatos), `grep` (búsqueda con posición), extracción de media, `stats` y `diff`. El mapa resuelve las decisiones de contrato y de cada funcionalidad; la implementación se ejecuta después, cuando el camino quede despejado.

## Notas

- **Dominio**: Rust, clap (derive), OxtIR. La CLI actual vive en `backend/src/main.rs` (~470 líneas) con `read/edit/create/info` planos y `google` anidado (`docs:read`, `sheets:read`, `slides:read`, `*:create`, `*:update`, `drive:list`, `drive:download`, `auth`). La librería (`lib.rs`, `google.rs`) ya abstrae local y Google detrás de OxtIR — el aplanamiento es mayormente superficie CLI.
- **Skills**: consultar `ponytail` (skill global) en toda sesión; `cuestioname` para interrogatorios; `arbolar` si se tocan CLAUDE.md.
- **Preferencias fijas** (decididas al trazar, 2026-07):
  - Flag global explícito `--json`; texto por defecto para humanos. Sin auto-detección TTY.
  - Errores: JSON en stderr + exit codes tipificados.
  - `-` = stdin en `path` y `--from`.
  - Subcomando `oxt schema` con el CLI en JSON.
  - **Aplanar todo**: un verbo lee/edita/crea cualquier origen (local o Google). Sin compatibilidad con la CLI actual; pi-office se actualiza después del deploy.
- **Investigación**: los tickets de investigación (AFK) se resuelven en sesión propia con herramientas web (`get_code_context_exa`, `web_search_exa`). No hay subagente `/research` en este entorno; los hallazgos se registran como respuesta en el ticket.
- La especificación del contrato vive en `docs/cli.md` — **v1 cerrada** (2026-07-30): árbol, orígenes, errores, outcomes, salida JSON por comando, schema, auth.
- **Ráfaga autorizada (2026-07-30)**: el usuario ordenó resolver la frontera y los 6 verbos en una sola sesión («hazlas en rafaga y luego arreglamos»). Resoluciones **revisadas en la misma sesión**: media con Google en v1 (sourceUri/contentUri), diff con lookahead 1, convert mapea legacy → OOXML; edit sin matches (exit 0 + changed:false) e internal_error en 10 confirmados.
- **Inventario hecho al trazar (2026-07)**: la CLI actual es `read` (9 formatos → text/markdown/ir/offset-map), `edit` (solo `--old/--new`), `create` (IR → 6 formatos), `info`, `google` anidado. El motor ya tiene lo que las features nuevas necesitan: roundtrip regenera XMLs desde IR (base de `update`), read→create es `convert`, `to_offset_map` es base de `grep`, `Element::Image` + ZIP crudo es base de media. Las features Google (`docs:update`, etc.) son el análogo existente para `update`.

## Decisiones hasta ahora

<!-- el índice — una línea por ticket cerrado -->

- [Forma plana de la CLI](tickets/forma-plana-de-la-cli.md) — árbol plano de 12 verbos + `schema` + `auth login|logout|status`; Google se expresa por el origen; `create --doc/--sheet/--slides`; `list`/`download` planos; matriz de orígenes (escritura a Google solo vía update/create); `read` por defecto markdown. Artefacto: `docs/cli.md`.
- [Convenciones de CLIs harness-first](tickets/convenciones-cli-harness-first.md) — clispec.dev: envelope `{kind, message, hint}` en stderr, outcomes sin envelope (grep/diff/auth status = 1), `changed` en mutantes, schema introspectable; gh: `--json` explícito. Adoptado en el contrato.
- [Detección de origen unificada](tickets/deteccion-de-origen-unificada.md) — módulo `origin.rs`: `-` → stdin; path existente → local; URL `/d/{id}` → Google con kind; ID desnudo (shape 25+ chars) → Google, kind vía `files.get`; resto → error `usage`.
- [Contrato de salida y errores JSON](tickets/contrato-de-salida-y-errores-json.md) — envelope de error siempre JSON en stderr (última línea); kinds: usage 2, io_error 3, unsupported_format/parse_error 4, invalid_ir 5, auth_error 6, api_error 7, edit_error 8, internal_error 10; payloads directos por comando; `read` = `{format, data}`.
- [oxt schema](tickets/oxt-schema.md) — `schema_version: 1`, generado en runtime caminando el árbol de clap (sin duplicar), tablas transversales en constante Rust; schema siempre JSON.
- [Auth headless para CI](tickets/auth-headless-para-ci.md) — device flow de Google muerto para Docs (solo scopes YouTube, verificado); inyección de refresh token: `OXT_GOOGLE_TOKEN` en env (en memoria) o `auth login --token` (persiste); `auth status` con `source: file|env|none`.
- [update local desde IR](tickets/update-local-desde-ir.md) — `update <origen> --from ir.json|-`; reemplaza todo el contenido, preservation bag; legacy convierte; salida local `{path, format, converted_from?}`, Google `{id, url}`.
- [convert entre formatos](tickets/convert-entre-formatos.md) — `convert <origen> <destino>`; destino entre los 6 writers; destino legacy escribe OOXML (como `create`); read→create ya es el pipeline; salida `{from, to, format}`.
- [grep con posición](tickets/grep-con-posicion.md) — `grep <patrón> <origen> [--literal] [-i]`; regex (crate `regex`); match = `{text, offset, path, context ±60}`; outcome 1 sin matches.
- [extracción de media](tickets/extraccion-de-media.md) — `media <origen> --output dir`; decodifica el base64 del `Element::Image`; dedupe con sufijo; local + Google (imágenes vía sourceUri/contentUri de Docs); `{files, skipped}`.
- [stats del documento](tickets/stats-del-documento.md) — `stats <origen> [--per-section]`; words = split_whitespace de plain_text; headings por nivel, hyperlinks; convive con `info`.
- [diff entre documentos](tickets/diff-entre-documentos.md) — `diff <a> <b>`; alineación por índice con lookahead 1 (inserciones detectadas); cambio = texto o formato; outcome 1 si hay diferencias; `{equal, changes}`.
- [Tests de integración de la CLI](tickets/tests-de-integracion-de-la-cli.md) — `backend/tests/cli.rs`: 12 tests de humo por verbo contra el binario (exit codes + formas JSON + stdin + outcomes). **Implementada y verde junto con la CLI.**

## Aún sin especificar

<!-- niebla: se gradúa a ticket cuando la pregunta sea nítida -->

- **Cobertura de tests de integración de la CLI**: qué nivel de tests por comando exige el contrato (humo `--json` por verbo). Se gradúa cuando el contrato de salida esté decidido.

*Nota: la niebla de tests se graduó al cerrar el contrato — ahora es el ticket [Tests de integración de la CLI](tickets/tests-de-integracion-de-la-cli.md).*

## Fuera de alcance

<!-- descartado conscientemente; nunca se gradúa -->

- **Extensión pi-office**: la CLI rota la rompe a propósito; se actualiza después del deploy. No es trabajo de este mapa.
- **Batching multi-archivo**: procesar N archivos por invocación.
- **Formatos nuevos** (PDF, Markdown como entrada, etc.): el contrato no los exige.
- **Interfaz no-CLI** (servidor, WASM, bindings): fuera del destino.
