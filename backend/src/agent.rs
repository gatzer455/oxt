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

/// Resultado de una operación de escritura.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WriteOutput {
    pub path: String,
    pub format: String,
    pub bytes_written: u64,
}

/// Resultado de una operación de edición.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EditOutput {
    pub path: String,
    pub replacements: usize,
    pub cells_set: usize,
}

/// Descripción de un formato para el agente.
pub const AGENT_SKILL: &str = r#"
# Xi Office — herramientas para documentos de oficina

## Comandos disponibles

### oxt read <archivo> [--format text|markdown|ir|offset-map]
Lee un documento y lo muestra en el formato indicado.
- text: texto plano (formato perdido)
- markdown: markdown con formato básico (default)
- ir: JSON estructurado (ideal para manipulación por LLM)
- offset-map: texto + mapa de rutas para ediciones precisas

### oxt edit <archivo> <viejo> <nuevo>
Reemplaza texto en un documento. Usa el TextOffsetMap
para localizar las ocurrencias.

### oxt create <archivo> --from <ir.json>
Crea un documento desde un archivo JSON con la estructura del IR.

### oxt info <archivo>
Muestra metadatos: formato, páginas, secciones, elementos.

## Formato del IR (JSON)

El IR (Intermediate Representation) es un JSON con esta estructura:
```json
{
  "sections": [{
    "title": "Nombre de sección",
    "elements": [
      { "kind": "heading", "level": 1, "text": "Título" },
      { "kind": "paragraph", "runs": [{ "text": "texto", "bold": true }] },
      { "kind": "table", "rows": [["A1", "B1"], ["A2", "B2"]] },
      { "kind": "list", "ordered": false, "items": ["item1", "item2"] }
    ]
  }]
}
```

## TextOffsetMap

Para ediciones precisas, se genera un mapa que asocia
cada span de texto con su ruta en el documento:
```json
{
  "full_text": "Texto completo del documento...",
  "spans": [
    { "start": 0, "end": 5, "path": "/s[0]/p[0]/r[0]", "text": "Texto" }
  ]
}
```
"#;
