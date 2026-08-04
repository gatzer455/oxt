---
titulo: stats del documento
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: [forma-plana-de-la-cli, contrato-de-salida-y-errores-json]
---

## Pregunta

¿Qué mide `oxt stats`, con qué granularidad, y cómo se ve su salida JSON y texto?

El IR ya permite contar todo por sección (`Section { title, elements }`) y por tipo de `Element` (Heading, Paragraph con runs, Table, List, Image, ThematicBreak).

Ramas:

- Métricas: secciones, elementos por tipo, párrafos, palabras, caracteres, tablas (¿filas/celdas?), listas (¿items?), imágenes, headings por nivel, runs con formato (¿bold/italic/hyperlink?).
- Granularidad: ¿totales del documento, o también por sección? ¿flag `--per-section` o siempre ambos?
- Semántica de "palabras": ¿split por whitespace sobre `plain_text`?
- Salida JSON: campos planos vs anidados por sección. Texto: tabla corta legible.
- Relación con `info` (que ya reporta secciones/elementos/título): ¿`stats` absorbe `info`, o conviven?

Criterio de salida: un harness obtiene métricas comparables entre documentos (útil para resumir/decidir tamaño de contexto).

## Resolución (2026-07-30, ráfaga autorizada — provisional)

Contrato en `docs/cli.md`:

- `oxt stats <origen> [--per-section] [--json]`.
- Métricas: sections, elements por tipo, paragraphs, words, characters, tables, table_rows, cells, list_items, images, headings por nivel, hyperlinks.
- words = `plain_text().split_whitespace().count()`; characters = chars de `plain_text()`.
- `--per-section` agrega `per_section: [{title, elements, words}]`.
- Texto: líneas alineadas `clave: valor`.
- `stats` no absorbe `info`: `info` queda como está (path/formato/estructura), `stats` es métricas. Ambos conviven.
- Implementación: función `OxtIR::stats()` en ir.rs (testeable).
