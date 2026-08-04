---
titulo: convert entre formatos
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: [forma-plana-de-la-cli, contrato-de-salida-y-errores-json]
---

## Pregunta

¿Cuál es el contrato del verbo `convert` —argumentos, pares de formatos válidos, salida JSON?

Costo casi nulo: `read` produce OxtIR y `create` lo escribe en cualquier formato soportado; el pipeline ya existe.

Hechos del motor: `create::create_from_ir(path, ir)` escribe según la extensión del destino (legacy mapea a OOXML). El reader de origen detecta por extensión con fallback (`.doc` real que es `.docx`).

Ramas:

- Argumentos: ¿`oxt convert <origen> <destino>` — el formato se infiere de la extensión del destino?
- Pares válidos: ¿todos los pares entre los 6 formatos de escritura? ¿origen legacy → destino OOXML? ¿destino legacy (¿se permite, mapea a OOXML? ¿o se rechaza y se exige extensión moderna?).
- Pérdida esperada: tabla de qué se pierde por par (formato inline solo sobrevive en formatos que lo soportan). ¿Se documenta en el contrato o se reporta en la salida?
- Salida JSON: campos (¿origen, destino, formato resultante, advertencias?).
- ¿`convert` y `create` comparten la semántica de escritura o son verbos separados que conviene unificar?

Criterio de salida: tabla de pares origen→destino con su comportamiento decidido, y la forma del verbo.

## Resolución (2026-07-30, ráfaga autorizada — provisional)

Contrato en `docs/cli.md`:

- `oxt convert <origen> <destino> [--json]` — formato de destino inferido de la extensión.
- Pares válidos: origen cualquier readable (local: 9 formatos; Google: doc/sheet/slides) → destino entre los 6 writers (docx/xlsx/pptx/odt/ods/odp).
- Destino legacy (.doc/.xls/.ppt) → escribe OOXML, igual que `create` (revisión 2026-07-30: consistencia — create ya mapea legacy a writers OOXML).
- Pérdida inherente por formato (formato inline en xlsx/ods) no se reporta como warning; el contrato la documenta.
- Implementación: `Document::open`/reader google → OxtIR → `create::create_from_ir(destino, ir)` (ya existe).
- Salida JSON: `{"from", "to", "format"}`.
