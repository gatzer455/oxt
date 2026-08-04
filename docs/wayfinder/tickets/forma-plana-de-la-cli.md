---
titulo: Forma plana de la CLI
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: []
---

## Pregunta

¿Cuál es el árbol exacto de la CLI plana —verbos top-level, posicionales, flags— que unifica local y Google en cada verbo, sin compatibilidad con la actual?

Hechos de partida (CLI actual en `backend/src/main.rs`): `read`, `edit`, `create`, `info` planos; `google` anidado con `auth`, `docs:read|create|update`, `sheets:read|create|update`, `slides:read|create|update`, `drive:list`, `drive:download`. La librería ya unifica detrás de OxtIR (`Document::open` local, `google.rs` por ID).

Ramas a cerrar (una por una, con `cuestioname`):

- Verbos top-level finales: ¿`read`, `edit`, `create`, `info`, `schema`, `auth`? ¿Qué pasa con `drive:list` y `drive:download` (¿verbos `list`/`download`? ¿se eliminan?).
- Cómo se expresa el origen/destino en cada verbo: URL de Google completa, ID desnudo, path local (ver ticket «Detección de origen unificada» para la semántica; acá se decide la superficie).
- `create` en modo Google: hoy `docs:create <title>` crea vacío. En la CLI plana, ¿`oxt create --title "X"`? ¿cómo se elige doc/sheet/slides? ¿`--type`? ¿o `create` solo escribe IR y Google se crea con otro mecanismo?
- `edit` en modo Google: ¿`oxt edit <doc>` con `--old/--new` igual que local? ¿`--from` para reemplazo total?
- `auth`: ¿top-level `oxt auth` (login/logout/status)? ¿flags `--client-id/--client-secret` se conservan?
- Posicionales vs flags: ¿`read <origen>` posicional y `--format` flag (como hoy)? ¿`edit` sigue con `--old/--new`?

Criterio de salida: se puede repetir la CLI completa —cada verbo con sus argumentos— sin ambigüedad.

## Resolución (2026-07-30, interrogatorio con el usuario)

Árbol plano final: `read`, `info`, `stats`, `grep`, `diff`, `edit`, `update`, `convert`, `create`, `media`, `list`, `download`, `schema`, `auth login|logout|status`. El namespace `google` y los nombres `docs:read` etc. desaparecen.

Decisiones:
- `grep <patrón> <origen>` — patrón primero, convención grep(1).
- `edit` conserva `--old/--new` como flags (textos con espacios).
- `create` modo Google: `--doc|--sheet|--slides "<título>"` (título como valor del flag, mutuamente excluyente con path local); `--from` opcional (sin él: doc vacío); salida id + url.
- `list` y `download` aplanados y conservados (ex `drive:list`/`drive:download`); `list` con `--query`; `download` para archivos no parseables.
- `auth login|logout|status` top-level; `--client-id/--client-secret` viven en `auth login`.
- Matriz de orígenes: read/info/stats/grep/diff/edit/update/media aceptan local + Google; `convert` escribe solo local; `create` destino local o Google por flag; list/download solo Google.
- `read` por defecto markdown.

Artefacto: `docs/cli.md` — especificación esqueleto del contrato con el árbol y la matriz.

Niebla graduada: «Textos por defecto» y «Features nuevas vs. orígenes Google» resueltos acá (markdown default; matriz). «Auth headless para CI» se graduó a ticket nuevo — quedó especificable al decidir la forma de `auth`.
