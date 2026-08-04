//! # Agent — salidas pensadas para el LLM
//!
//! Formatos de salida que el agente (pi) puede consumir directamente:
//!   - Markdown (legible, para contexto)
//!   - JSON IR (estructurado, para manipulación)
//!   - TextOffsetMap (preciso, para ediciones quirúrgicas)

use crate::ir::*;
use serde_json;

/// Resultado de una operación de lectura, en múltiples formatos.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReadOutput {
    /// Texto plano (pérdida de formato).
    pub plain_text: String,

    /// Markdown (con formato básico).
    pub markdown: String,

    /// IR completo como JSON.
    pub ir_json: serde_json::Value,

    /// Mapa de offsets para ediciones precisas.
    pub offset_map: TextOffsetMap,

    /// Metadatos
    pub format: String,
    pub sections: usize,
    pub elements: usize,
}

impl ReadOutput {
    pub fn from_ir(ir: &OxtIR, format: &str) -> Self {
        let element_count: usize = ir.sections.iter()
            .map(|s| s.elements.len())
            .sum();

        Self {
            plain_text: ir.plain_text(),
            markdown: ir.to_markdown(),
            ir_json: serde_json::to_value(ir).unwrap_or_default(),
            offset_map: ir.to_offset_map(format),
            format: format.to_string(),
            sections: ir.sections.len(),
            elements: element_count,
        }
    }
}

/// Descripción de la CLI para el agente (contrato v1, ver docs/cli.md).
pub const AGENT_SKILL: &str = r#"# oxt — documentos de oficina para LLMs

Un verbo, cualquier origen: cada comando acepta un path local, una URL de Google (`https://docs.google.com/document/d/ID/edit`) o un ID de Google. `-` = stdin.

## Comandos

- `oxt read <origen> [--format text|markdown|ir|offset-map] [--json]` — leer. Default markdown.
- `oxt info <origen> [--json]` — formato, secciones, elementos, título.
- `oxt stats <origen> [--per-section] [--json]` — métricas (palabras, tablas, imágenes…).
- `oxt grep <patrón> <origen> [--literal] [-i] [--json]` — regex; matches con offset y ruta IR. Exit 1 si no hay matches.
- `oxt diff <a> <b> [--json]` — compara dos documentos (cualquier formato). Exit 1 si hay diferencias.
- `oxt edit <origen> --old "x" --new "y" [--json]` — reemplazo in-place (preserva estilos). `changed: false` si no hubo reemplazos.
- `oxt update <origen> --from ir.json|- [--json]` — reemplaza TODO el contenido con un IR.
- `oxt convert <origen> <destino> [--json]` — convierte entre formatos (destino local).
- `oxt create <path> --from ir.json|- [--json]` — crea local; `oxt create --doc|--sheet|--slides "Título" [--from -]` — crea en Google.
- `oxt media <origen> --output dir [--json]` — extrae imágenes (base64 del IR o sourceUri de Google).
- `oxt list [--query] [--json]` / `oxt download <id|url> --output <path> [--json]` — Google Drive.
- `oxt schema` — JSON con toda la CLI (comandos, flags, errores, exit codes).
- `oxt auth login|logout|status` — Google OAuth; CI: env `OXT_GOOGLE_TOKEN` o `auth login --token`.

## Contrato de salida

- `--json` → JSON en stdout (stdout solo datos). Texto por defecto.
- Errores: JSON en stderr, última línea: `{"kind": ..., "message": ...}`. Exit codes: 2 usage, 3 io, 4 formato, 5 IR inválido, 6 auth, 7 api, 8 edit, 10 interno.
- Outcomes (exit ≠ 0 sin error): grep sin matches, diff con diferencias, auth status sin sesión → 1.
- Formatos: docx/xlsx/pptx/odt/ods/odp/doc/xls/ppt (legacy: lectura y conversión, edición convierte a OOXML).

## Formato del IR (JSON)

```json
{
  "sections": [{
    "title": "Nombre",
    "elements": [
      {"kind": "heading", "level": 1, "text": "Título"},
      {"kind": "paragraph", "runs": [{"text": "texto", "bold": true}]},
      {"kind": "table", "rows": [["A1", "B1"]]},
      {"kind": "list", "ordered": false, "items": ["item"]},
      {"kind": "image", "filename": "foto.png", "data": "<base64>", "alt_text": "..."}
    ]
  }]
}
```

Los paths IR (`/s[0]/p[1]/r[2]`) que devuelven `grep`/`diff`/`offset-map` localizan cada texto con precisión."#;
