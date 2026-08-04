---
titulo: Tests de integración de la CLI
tipo: wayfinder:tarea
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: []
---

## Pregunta

¿Qué nivel de tests de integración exige el contrato para la CLI implementada?

Graduado de la niebla al cerrar el contrato: la forma JSON por comando y los exit codes ya están decididos (`docs/cli.md`); falta la cobertura que verifique que la CLI se comporta como el contrato dice.

Trabajo:

- Un test de humo por verbo: invocar el binario real (vía `env!("CARGO_BIN_EXE_oxt")` o `assert_cmd`) y verificar exit code + forma del JSON en stdout + envelope en stderr.
- Casos mínimos por comando: `read` (text/markdown/ir/offset-map, `-` por stdin), `info`, `stats`, `grep` (match y outcome 1 sin matches), `diff` (igual y outcome 1), `edit` (replacements/changed), `update` (local), `convert`, `create` (local), `media`, `schema` (JSON parseable con schema_version), errores (archivo inexistente → 3/io_error, IR inválido → 5/invalid_ir, clap → 2/usage).
- No cubrir Google (requiere red/tokens): los verbos google se prueban en la fase de integración manual.

Criterio de salida: `cargo test` verde con la CLI cubierta, sin red.

## Resolución (2026-07-30, ejecutada con la implementación)

`backend/tests/cli.rs` — 12 tests de integración contra el binario real (`CARGO_BIN_EXE_oxt`), sin red:

- create+read (json, ir), read por stdin con sniffing, info, stats, grep (match + outcome 1 + --literal), edit (changed true/false), update (archivo y stdin), convert, diff (outcome 1 + iguales), media (docx con drawing → PNG real), schema (parseable, verbos presentes), envelopes de error (usage 2, unsupported_format 4, invalid_ir 5, clap 2), help plano sin `docs:read`.

Nota de implementación: `--help`/`--version` de clap salen por `try_parse` — se detecta el kind `DisplayHelp`/`DisplayVersion` y se emite a stdout con exit 0 (no es un error).
