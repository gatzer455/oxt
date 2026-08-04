---
titulo: grep con posición
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: [forma-plana-de-la-cli, contrato-de-salida-y-errores-json]
---

## Pregunta

¿Cuál es el contrato del verbo `grep` —patrón, qué se reporta, salida JSON?

Hechos del motor: `ir::to_offset_map(format)` ya produce el `TextOffsetMap` que relaciona cada span de texto con su ruta en el documento (`/s[0]/p[1]/r[2]`). `read --format offset-map` lo expone. Falta la búsqueda sobre ese mapa.

Ramas:

- Patrón: ¿regex o literal? ¿flag `--literal`? ¿`-i` case-insensitive? ¿flag `-v` invertir? (¿o mínimo viable: literal + regex, sin invertir?)
- Qué se reporta por match: texto, offset, ruta IR (sección/elemento/run), contexto (¿línea/segmento circundante?).
- Formato de salida: JSON (lista de matches) y texto (¿grep clásico `archivo:offset:texto` o similar?).
- Semántica con múltiples coincidencias por run y runs con formato partido: ¿cómo se agrupa?
- Salida JSON: campos por match y envoltura (según «Contrato de salida y errores JSON»).

Criterio de salida: un harness sabe invocar `grep` y parsear los matches con su ubicación para editar después con precisión.

## Resolución (2026-07-30, ráfaga autorizada — provisional)

Contrato en `docs/cli.md`:

- `oxt grep <patrón> <origen> [--literal] [-i] [--json]` — regex por defecto (crate `regex`), `--literal` para búsqueda textual, `-i` case-insensitive.
- Búsqueda sobre el texto plano del IR (runs concatenados); match → `{"text", "offset", "path", "context"}`: offset = índice de chars sobre `plain_text`, path = ruta IR `/s[i]/p[j]/r[k]`, context = ventana ±60 chars.
- Multiple matches por run: uno por match. Sin `-v` en v1 (YAGNI).
- **Outcome**: exit 1 si no hay matches (sin envelope de error, stdout con `{"matches": []}` en --json); exit 0 con matches.
- Texto (sin --json): `origen:offset:texto` por línea.
- Implementación: extender `to_offset_map`/recorrer IR; nueva función en ir.rs o grep.rs.
