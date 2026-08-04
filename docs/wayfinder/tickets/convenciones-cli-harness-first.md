---
titulo: Convenciones de CLIs harness-first
tipo: wayfinder:investigacion
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: []
---

## Pregunta

¿Cómo estructuran las CLIs pensadas para scripting su salida JSON, errores y exit codes? Recomendaciones concretas para el envelope de oxt.

Resolver en sesión propia (AFK) con herramientas web (`get_code_context_exa`, `web_search_exa`). No hay subagente `/research`; los hallazgos se registran como respuesta en este ticket.

Casos a examinar:

- **gh CLI** (`--json` con `--jq`, envelope de salida, errores en stderr).
- **docker CLI** (formato JSON de errores, exit codes documentados).
- **CLIs de agentes** (claude code, aichat, etc.): cómo estructuran salidas que un LLM parsea.
- Convenciones Unix (sysexits) y de clap para exit codes (2 = argumentos).
- Cualquier patrón de envelope de error con código estable + mensaje.

Entregable: recomendaciones concretas (con ejemplos citados) para (a) envelope de éxito, (b) envelope de error, (c) tabla de exit codes, (d) qué evita cada decisión. Alimenta «Contrato de salida y errores JSON».

## Resolución (2026-07-30, investigación con web_search/get_code_context)

Hallazgos adoptados en el contrato (`docs/cli.md`):
- **CLI Spec (clispec.dev)**: envelope de error `{kind, message, hint?}` como última línea de stderr; `kind` estable declarado en el schema; stdout=datos/stderr=diagnóstico; **outcomes** (exit ≠ 0 que señalan estado de datos, no fallo — sin envelope): aplicado a grep (1 = sin matches), diff (1 = hay diferencias), auth status (1 = no autenticado); `changed: bool` en comandos mutantes (Terraform/Ansible); schema introspectable (valida `oxt schema`); prohibición de que outcomes y errores compartan exit code → `internal_error` en 10, outcomes en 1.
- **gh CLI**: `--json` explícito con fields; texto por defecto sin ANSI al no ser TTY. No se adopta la auto-detección TTY (decisión previa del mapa: flag explícito), ni `--jq`/`--template` (fuera de alcance).
- **sysexits**: no se adopta; se mantiene 2 de clap y tabla propia 3-10.

Decisión extra: los errores de clap se re-emiten como envelope `usage` vía `try_parse()` en main, para que stderr sea JSON en todos los errores.
