---
titulo: diff entre documentos
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: [forma-plana-de-la-cli, contrato-de-salida-y-errores-json]
---

## Pregunta

¿Cuál es el contrato del verbo `diff` —argumentos, qué se compara, cómo se reportan las diferencias?

Comparar dos documentos a través de su OxtIR normalizado (mismo formato de IR, independiente del formato de archivo: `a.docx` vs `b.odt` son comparables).

Ramas:

- Argumentos: ¿`oxt diff <origen-a> <origen-b>`? (¿algún día uno de los lados por stdin — `-`?)
- Qué se compara: ¿IR completo elemento por elemento (título de sección, texto, formato de runs, celdas, items)? ¿o solo texto plano (ignora formato)?
- Reporte: lista de cambios con ruta IR (`/s[0]/p[2]`), tipo (agregado/eliminado/modificado), y detalle (texto antes/después o resumen).
- Algoritmo: ¿alineación por índice (mismo índice de sección/elemento) o por similitud? — el más simple que sirva a un harness: alineación por índice con marca de agregados/eliminados.
- Salida: JSON estructurado (cambios) y texto (¿estilo unified diff sobre el markdown?).
- Diferencias de formato puro (mismo texto, bold distinto): ¿cuentan como cambio?

Criterio de salida: un harness compara dos versiones de un doc y obtiene la lista de cambios con ubicación, sin leer el diff humano.

## Resolución (2026-07-30, ráfaga autorizada — provisional)

Contrato en `docs/cli.md`:

- `oxt diff <origen-a> <origen-b> [--json]` — ambos lados local o Google; comparación sobre OxtIR normalizado (formatos distintos comparables).
- Alineación por índice con **lookahead de 1** (revisión 2026-07-30): si el elemento i difiere pero el i+1 de B coincide con el i de A → inserción (added), no modified en cascada. Sin LCS. Cambio = texto distinto O formato de runs distinto; `type: added|removed|modified`; `old`/`new` = representación textual del elemento.
- Cambio = texto distinto O formato de runs distinto; `type: added|removed|modified`; `old`/`new` = representación textual del elemento.
- **Outcome**: exit 1 si hay diferencias, 0 si iguales (stdout siempre lleva `{"equal", "changes"}` en --json; sin envelope de error).
- Texto: líneas `path: - old / + new`.
- Implementación: función `diff_ir(a, b)` en ir.rs (testeable); paths estilo `/s[i]/p[j]`.
