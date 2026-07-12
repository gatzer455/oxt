#![allow(unused_assignments, unused_variables)]
//! # Edit — modificar documentos in-place
//!
//! Abre un OOXML (DOCX/XLSX/PPTX) como ZIP, reemplaza texto
//! en las partes XML relevantes, y escribe el ZIP de vuelta.
//!
//! Estrategia: modificación directa del XML serializado.
//! Simple, predecible, preserva el resto del paquete intacto.


use std::path::Path;

/// Error del módulo edit.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Formato no soportado para edición: {0}")]
    UnsupportedFormat(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, EditError>;

/// Resultado de una operación de edición.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EditResult {
    pub path: String,
    pub replacements: usize,
    pub affected_parts: Vec<String>,
}

/// Reemplazar texto recursivamente en todo el OxtIR.
pub(crate) fn replace_in_ir(ir: &mut crate::ir::OxtIR, old: &str, new: &str) -> usize {
    let mut count = 0;
    for section in &mut ir.sections {
        for element in &mut section.elements {
            match element {
                crate::ir::Element::Heading { text, .. } => {
                    count += text.matches(old).count();
                    *text = text.replace(old, new);
                }
                crate::ir::Element::Paragraph { runs } => {
                    for run in runs {
                        count += run.text.matches(old).count();
                        run.text = run.text.replace(old, new);
                    }
                }
                crate::ir::Element::List { items, .. } => {
                    for item in items {
                        count += item.matches(old).count();
                        *item = item.replace(old, new);
                    }
                }
                crate::ir::Element::Table { rows } => {
                    for row in rows {
                        for cell in row {
                            count += cell.matches(old).count();
                            *cell = cell.replace(old, new);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    count
}


/// Reemplazar texto en un documento OOXML (DOCX/XLSX/PPTX).
///
/// Args:
/// - `path`: ruta al archivo
/// - `old`: texto a reemplazar
/// - `new`: texto nuevo
///
/// Returns: número de reemplazos realizados.
pub fn replace_text(path: impl AsRef<Path>, old: &str, new: &str) -> Result<EditResult> {
    let path = path.as_ref();
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    // Lista de partes a modificar según el formato
    let parts: &[&str] = match ext.as_str() {
        "docx" => &[
            "word/document.xml",
            "word/header1.xml",
            "word/header2.xml",
            "word/footer1.xml",
            "word/footer2.xml",
        ],
        "xlsx" => &[
            "xl/sharedStrings.xml",
            // sheets se detectan dinámicamente
        ],
        "pptx" => &[
            // slides se detectan dinámicamente
        ],
        "odt" | "ods" | "odp" => &[
            "content.xml",
        ],
        "doc" | "xls" | "ppt" => {
            // Legacy binary: leer→OxtIR→reemplazar→escribir como OOXML
            return edit_legacy_via_ir(path, &ext, old, new);
        }
        _ => return Err(EditError::UnsupportedFormat(ext)),
    };

    // Usar RoundtripDoc para OOXML y ODF (preservation bag)
    // Esto mantiene TODAS las partes intactas y solo regenera
    // el contenido principal (document.xml / content.xml) desde el OxtIR.
    let doc = crate::roundtrip::RoundtripDoc::open(path)
        .map_err(|e| EditError::Other(format!("Error al abrir: {e}")))?;

    let mut ir = doc.ir.clone();
    let mut replacements = 0;

    eprintln!("DEBUG: IR sections={}, plain={:?}", ir.sections.len(), ir.plain_text().chars().take(100).collect::<String>());

replacements = replace_in_ir(&mut ir, old, new);

    if replacements == 0 {
        return Ok(EditResult {
            path: path.to_string_lossy().to_string(),
            replacements: 0,
            affected_parts: vec![],
        });
    }

    // Guardar con preservation bag (solo regenera el contenido principal)
    // No podemos usar replace_text_and_save porque necesita &mut self
    // y hemos movido `doc`. En su lugar, creamos un nuevo RoundtripDoc modificado.
    let modified = crate::roundtrip::RoundtripDoc {
        ir,
        format: doc.format,
        path: doc.path,
        parts: doc.parts,
        regenerated_indices: doc.regenerated_indices,
    };
    modified.save(path)
        .map_err(|e| EditError::Other(format!("Error al guardar: {e}")))?;

    Ok(EditResult {
        path: path.to_string_lossy().to_string(),
        replacements,
        affected_parts: vec![format!("regenerado via OxtIR ({ext})")],
    })
}



/// Cuenta ocurrencias de un substring.
/// Editar un documento legacy leyéndolo a OxtIR, reemplazando texto,
/// y escribiendo como OOXML (.doc → .docx, .xls → .xlsx, .ppt → .pptx).
fn edit_legacy_via_ir(path: &Path, ext: &str, old: &str, new: &str) -> Result<EditResult> {
    use crate::create;
    use crate::Document;

    // Leer el documento legacy a OxtIR
    let doc = Document::open(path)
        .map_err(|e| EditError::Other(format!("Error al leer legacy: {e}")))?;
    let mut ir = doc.into_ir();

    // Contar y reemplazar texto en todos los runs del IR
    let mut replacements = 0;


    replacements = replace_in_ir(&mut ir, old, new);

    if replacements == 0 {
        return Ok(EditResult {
            path: path.to_string_lossy().to_string(),
            replacements: 0,
            affected_parts: vec![],
        });
    }

    // Determinar extensión OOXML de salida
    let out_ext = match ext {
        "doc" => "docx",
        "xls" => "xlsx",
        "ppt" => "pptx",
        _ => unreachable!(),
    };

    let out_path = path.with_extension(out_ext);

    // Escribir como OOXML
    create::create_from_ir(&out_path, &ir)
        .map_err(|e| EditError::Other(format!("Error al escribir {out_ext}: {e}")))?;

    // Reportar
    Ok(EditResult {
        path: out_path.to_string_lossy().to_string(),
        replacements,
        affected_parts: vec![format!("convertido de .{ext} a .{out_ext}")],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_replace_text_in_docx() {
        use std::fs;
        use zip::write::SimpleFileOptions;

        // Crear un DOCX mínimo para probar
        let dir = std::env::temp_dir().join("oxt_test_edit");
        let _ = fs::create_dir_all(&dir);
        let docx_path = dir.join("test_edit.docx");

        // Crear un ZIP que simule un DOCX
        let file = fs::File::create(&docx_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        // Content Types
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#).unwrap();

        // Relationships
        zip.start_file("_rels/.rels", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#).unwrap();

        // Document
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r>
        <w:t>Hello World</w:t>
      </w:r>
    </w:p>
  </w:body>
</w:document>"#).unwrap();

        zip.finish().unwrap();

        // Probar reemplazo (ahora vía RoundtripDoc)
        let result = replace_text(&docx_path, "World", "oxt").unwrap();
        assert_eq!(result.replacements, 1);
        assert!(result.affected_parts[0].contains("regenerado"),
            "debe indicar regeneración, obtuve: {:?}", result.affected_parts);

        // Verificar el contenido vía Document::open
        let doc = crate::Document::open(&docx_path).unwrap();
        let text = doc.plain_text();
        assert!(text.contains("Hello oxt"), "debe contener texto reemplazado, obtuve: {text:?}");
        assert!(!text.contains("Hello World"), "no debe contener texto original");

        // Limpiar
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_edit_legacy_doc_converts_to_docx() {
        let dir = std::env::temp_dir().join("oxt_test_legacy_edit");
        let _ = std::fs::create_dir_all(&dir);
        let doc_path = dir.join("test_legacy.doc");
        let docx_path = dir.join("test_legacy.docx");

        let ir = crate::ir::OxtIR {
            metadata: crate::ir::Metadata::default(),
            sections: vec![
                crate::ir::Section {
                    title: None,
                    elements: vec![
                        crate::ir::Element::Paragraph {
                            runs: vec![crate::ir::Run::plain("Texto legacy para editar")],
                        },
                    ],
                },
            ],
        };

        crate::create::create_from_ir(&doc_path, &ir).unwrap();
        assert!(doc_path.exists());

        let result = super::replace_text(&doc_path, "legacy", "convertido").unwrap();
        assert_eq!(result.replacements, 1);
        assert!(result.affected_parts[0].contains("convertido"));

        assert!(docx_path.exists());
        let doc = crate::Document::open(&docx_path).unwrap();
        let text = doc.plain_text();
        assert!(text.contains("convertido"));
        assert!(!text.contains("legacy"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
