# Contrato CLI de oxt

Especificación del contrato de la CLI para cualquier harness (agentes LLM, scripts). **v1 cerrada (2026-07-30)** — todas las decisiones tomadas en el tracker `docs/wayfinder/mapa.md`; falta solo la implementación. Documento canónico: este archivo.

## Principios

1. **Un verbo, cualquier origen**: `read`, `edit`, `update`, etc. aceptan path local o documento de Google (URL o ID). Google se expresa por el origen, nunca por el verbo.
2. **Salida por defecto legible por humanos** (español); `--json` explícito para máquinas. Sin auto-detección TTY. Sin ANSI cuando no es TTY.
3. **stdout = datos, stderr = diagnóstico**. Con `--json`, stdout lleva SOLO el JSON del resultado (nunca "Creado: ..."). Los errores van siempre a stderr.
4. **Errores**: JSON en stderr (última línea), exit codes tipificados. Los **outcomes** (datos de estado) usan exit ≠ 0 sin envelope de error.
5. **`-` = stdin** en `path` y `--from`.
6. **Sin compatibilidad** con la CLI pre-contrato (`oxt google docs:read`, `oxt google auth`, …).
7. **Claves JSON en inglés** (identificadores estables), mensajes en español. Los `kind` de error son estables y se declaran en `oxt schema`.
8. Comandos que mutan incluyen `changed: bool` (convención Terraform/Ansible) cuando la distinción importa (edit, update).

## Árbol de verbos

| Verbo | Posicionales | Flags | Origen | Destino |
|---|---|---|---|---|
| `read` | `<origen>` | `--format text\|markdown\|ir\|offset-map` (default **markdown**), `--json` | local + Google | — |
| `info` | `<origen>` | `--json` | local + Google | — |
| `stats` | `<origen>` | `--per-section`, `--json` | local + Google | — |
| `grep` | `<patrón>` `<origen>` | `--literal`, `-i`, `--json` | local + Google | — |
| `diff` | `<origen-a>` `<origen-b>` | `--json` | local + Google (ambos lados) | — |
| `edit` | `<origen>` | `--old`, `--new`, `--json` | local + Google | in-place |
| `update` | `<origen>` | `--from <ir.json\|->`, `--json` | local + Google | in-place |
| `convert` | `<origen>` `<destino>` | `--json` | local + Google | solo local (destino legacy → OOXML) |
| `create` | `<destino-local>` | `--from <ir.json\|->`, `--json` | — | local |
| `create` | — | `--doc\|--sheet\|--slides "<título>"`, `--from` opcional, `--json` | — | Google |
| `media` | `<origen>` | `--output <dir>` (default: `media/`), `--json` | local + Google | dir local |
| `list` | — | `--query`, `--json` | solo Google (Drive) | — |
| `download` | `<id\|url>` | `--output <path>`, `--json` | solo Google (Drive) | archivo |
| `schema` | — | — | — | siempre JSON |
| `auth` | `login\|logout\|status` | `login`: `--client-id`, `--client-secret`, `--token` | — | — |

Convenciones de forma: `grep <patrón> <origen>` (patrón primero, grep(1)); `edit --old/--new` como flags (textos con espacios); `create` modo Google con título como valor del flag, mutuamente excluyente con path local; `read` por defecto markdown.

## Orígenes (detección)

Módulo nuevo `origin.rs` en backend. Precedencia de `resolve_origin(&str) -> Result<Origin>`:

1. `-` → `Origin::Stdin`
2. Existe como path local (`fs::exists`) → `Origin::Local`
3. URL `http(s)://docs.google.com/{document|spreadsheets|presentation|file}/d/{id}` o `drive.google.com/file/d/{id}` → `Origin::Google { id, kind }` (kind explícito de la URL)
4. Shape de ID de Google `^[A-Za-z0-9_-]{25,}$` → `Origin::Google { id, kind: None }` — kind resuelto en runtime vía `files.get` de Drive API (una llamada, solo para IDs desnudos)
5. Ninguna → error `usage`, hint con ejemplos

`Origin::Google { kind: None }` se resuelve al primer uso: doc/sheet/slides según mimeType.

## Errores (stderr, siempre JSON)

Envelope (última línea de stderr, una sola línea JSON):

```json
{"kind": "io_error", "message": "No se pudo abrir el archivo", "hint": "¿Existe la ruta?"}
```

| kind | exit | uso |
|---|---|---|
| `usage` | 2 | argumentos inválidos (clap via `try_parse`) u origen no reconocido |
| `io_error` | 3 | archivo inexistente, no legible, no escribible |
| `unsupported_format` | 4 | extensión de origen/destino no soportada |
| `parse_error` | 4 | documento corrupto o ilegible |
| `invalid_ir` | 5 | `--from` no es OxtIR válido |
| `auth_error` | 6 | no autenticado o credenciales rechazadas |
| `api_error` | 7 | error de red o de API de Google |
| `edit_error` | 8 | edición fallida (ej. legacy sin conversión posible) |
| `internal_error` | 10 | fallback genérico (sin kind más específico) |

Los errores de clap salen por `Cli::try_parse()` y se re-emiten con el envelope `usage`.

## Outcomes (exit ≠ 0, sin envelope, stdout lleva el resultado)

| comando | exit | significado |
|---|---|---|
| `grep` | 1 | sin matches (stdout: matches vacío o nada en texto) |
| `diff` | 1 | hay diferencias (stdout: el JSON/reporte igual) |
| `auth status` | 1 | no autenticado |

No se solapan con los códigos de error: `internal_error` ocupa el 10 para dejar el 1 libre.

## Salida JSON por comando (`--json`)

| Verbo | Payload en stdout |
|---|---|
| `read` | `{"format": "<pedido>", "data": <IR \| string \| offset-map>}` — data es el objeto IR para `ir`, el string para `text`/`markdown`, el mapa para `offset-map` |
| `info` | `{"path", "format", "sections", "elements", "title"}` |
| `stats` | `{"sections", "elements": {por tipo}, "paragraphs", "words", "characters", "tables", "table_rows", "cells", "list_items", "images", "headings": {por nivel}, "hyperlinks"}` + `"per_section": [{title, elements, words}]` solo con `--per-section` |
| `grep` | `{"matches": [{"text", "offset", "path", "context"}]}` — offset = chars sobre `plain_text`; path = ruta IR `/s[i]/p[j]/r[k]`; context = ventana ±60 chars |
| `diff` | `{"equal": bool, "changes": [{"path", "type": "added\|removed\|modified", "old", "new"}]}` — alineación por índice con lookahead 1 (inserciones detectadas); `modified` = texto o formato distinto |
| `edit` | `{"replacements": n, "changed": bool, "affected_parts": [...]}` |
| `update` | local: `{"path", "format", "converted_from"?}`; Google: `{"id", "url"}` |
| `convert` | `{"from", "to", "format"}` |
| `create` | local: `{"path"}`; Google: `{"id", "url"}` |
| `media` | `{"files": [{"file", "filename", "ir_path", "bytes"}], "skipped": [{"ir_path", "reason"}]}` — en Google, imágenes vía sourceUri/contentUri de Docs (doc/slides); las que no resuelven van a `skipped` |
| `list` | `[{"id", "name", "mime", "modified"}]` (array directo) |
| `download` | `{"path"}` |
| `auth login` / `auth logout` | `{"status": "ok"}` |
| `auth status` | `{"authenticated": bool, "source": "file\|env\|none"}` (exit 0/1 según `authenticated`) |
| `schema` | el objeto schema (ver abajo) |

## oxt schema

`schema_version: 1`. Generado en runtime caminando el árbol de clap (sin duplicar definiciones); las tablas transversales viven en una constante Rust.

```json
{
  "schema_version": 1,
  "tool": "oxt",
  "version": "<semver>",
  "output": {"tty": "text", "piped": "text"},
  "global_args": [{"name": "--json", "description": "Salida estructurada JSON"}],
  "commands": [{"name", "description", "arguments": [{"name", "required", "takes_value", "default", "choices"}], "mutates": bool}],
  "errors": [{"kind", "exit"}],
  "outcomes": [{"code", "meaning"}],
  "formats": ["text", "markdown", "ir", "offset-map"],
  "extensions": ["docx", "xlsx", "pptx", "odt", "ods", "odp", "doc", "xls", "ppt"]
}
```

## Auth

`oxt auth login` — flujo OAuth desktop (PKCE + loopback), credenciales embebidas o `--client-id/--client-secret` propias. `oxt auth logout` — borra `~/.config/oxt/google-tokens.json`. `oxt auth status` — reporta estado.

**Headless (CI)**: el device flow de Google NO sirve (solo scopes de YouTube, verificado 2026-07). Mecanismo: **inyección de refresh token**:
- `OXT_GOOGLE_TOKEN=<refresh_token>` en env → se usa en memoria para la invocación, sin tocar el archivo.
- `oxt auth login --token <refresh_token>` → lo guarda en el archivo (para tokens obtenidos out-of-band).
- Flujo documentado para CI: generar el token una vez con `oxt auth login` en una máquina con navegador → copiar el refresh token al secret store del CI → `OXT_GOOGLE_TOKEN` por invocación.
- `auth status` distingue `source: file|env|none`.

## Límites conocidos (v1)

- `diff` alinea por índice con lookahead de 1 (detección de inserciones por coincidencia con el siguiente elemento); sin LCS.
- `media` en Google depende de sourceUri/contentUri de la API de Docs: URLs que expiran o faltan (docs sin imágenes) van a `skipped` con reason. Sheets no tienen imágenes.
- `convert` con destino .doc/.xls/.ppt escribe OOXML, igual que `create` (el contenido es moderno, solo cambia la extensión).
- La pérdida de formato inline en destinos tipo hoja de cálculo (xlsx/ods) es inherente al formato, no se reporta como advertencia.
- **Fidelidad del writer DOCX** (pre-existente, no bloquea el contrato): `create` no escribe `styles.xml` ni `numbering.xml`, así que headings y listas se leen de vuelta como párrafos; tampoco escribe imágenes (`Element::Image` se descarta al crear). La lectura de imágenes de DOCX reales sí funciona (`w:drawing` → base64).

## Pendiente de implementación

Ver `docs/wayfinder/tickets/tests-de-integracion-de-la-cli.md` — un test de humo por verbo (exit code + forma JSON), ejecutado en la fase de implementación.
