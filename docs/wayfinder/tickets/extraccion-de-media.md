---
titulo: extracción de media
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: [forma-plana-de-la-cli, contrato-de-salida-y-errores-json]
---

## Pregunta

¿Cuál es el contrato del verbo para extraer media (imágenes y otros binarios) de un documento —argumentos, qué se extrae, cómo se nombra, salida JSON?

Hechos del motor: el IR tiene `Element::Image`; los binarios viven como entries del ZIP (el roundtrip los preserva como bytes crudos). No hay hoy ningún comando que los exporte.

Ramas:

- Superficie: ¿verbo propio (`oxt media <origen> --output dir`)? ¿subcomando de `read`? ¿flag de `read`?
- Qué se extrae: ¿solo imágenes referenciadas por el IR, o todos los binarios del ZIP (fuentes, charts)? ¿también los de docs de Google (requiere descarga vía API — ¿se soporta o solo local)?
- Nombrado: ¿nombre original del ZIP (`media/image1.png`), deduplicación, o nombre derivado de la ruta IR?
- Mapeo de vuelta: ¿la salida JSON relaciona cada archivo extraído con su `Element::Image` (ruta IR) para que un harness sepa qué imagen es cuál?
- ¿Flag de sobrescritura de archivos existentes en `--output`?

Criterio de salida: un harness extrae las imágenes de un doc y sabe cuál corresponde a cada referencia del IR.

## Resolución (2026-07-30, ráfaga autorizada — provisional)

Contrato en `docs/cli.md`:

- `oxt media <origen> --output <dir> [--json]` — default `--output media/`.
- Extracción: walk del IR → `Element::Image { filename, data (base64), alt_text }` → decodificar `data` a archivo. No requiere tocar el ZIP (el reader ya embebe base64 en el IR).
- Nombrado: `filename` del IR; colisiones → sufijo numérico (`image1_2.png`). Sin `data` → entra a `skipped` con reason.
- Salida JSON: `{"files": [{"file", "filename", "ir_path", "bytes"}], "skipped": [{"ir_path", "reason"}]}`.
- **v1 incluye Google** (revisión 2026-07-30): para orígenes Google, las imágenes se traen vía sourceUri/contentUri de la API de Docs (inlineObjects de `read_doc`); las que no resuelven (URL expirada, sin imagen) van a `skipped` con reason. Sheets no tienen imágenes (lista vacía).
- Implementación: función nueva en create.rs? No — módulo propio `media.rs` o función en ir.rs; decisión de implementación.
