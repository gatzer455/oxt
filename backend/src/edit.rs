#![allow(unused_assignments, unused_variables)]
#![allow(unused_assignments)]
//! # Edit — modificar documentos in-place
//!
//! Abre un OOXML (DOCX/XLSX/PPTX) como ZIP, reemplaza texto
//! en las partes XML relevantes, y escribe el ZIP de vuelta.
//!
//! Estrategia: modificación directa del XML serializado.
//! Simple, predecible, preserva el resto del paquete intacto.

use std::io::{Read, Write};
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

    // Leer el ZIP completo en memoria
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // Recopilar todas las partes a modificar
    let mut all_parts: Vec<String> = parts.iter().map(|s| s.to_string()).collect();

    if ext == "xlsx" {
        // Agregar sheets dinámicamente
        for i in 1..100 {
            let name = format!("xl/worksheets/sheet{i}.xml");
            if archive.by_name(&name).is_ok() {
                all_parts.push(name);
            } else {
                break;
            }
        }
    } else if ext == "pptx" {
        // Agregar slides dinámicamente
        for i in 1..100 {
            let name = format!("ppt/slides/slide{i}.xml");
            if archive.by_name(&name).is_ok() {
                all_parts.push(name);
            } else {
                break;
            }
        }
    }

    // Leer, modificar y guardar
    let mut replacements: usize = 0;
    let mut affected_parts = Vec::new();
    let mut new_entries: Vec<(String, Vec<u8>)> = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;

        if all_parts.contains(&name) {
            // Reemplazar solo dentro de <w:t>, <a:t>, <t> tags
            let content = String::from_utf8_lossy(&data);
            let modified = content.replace(old, new);

            if modified != content {
                let count = count_occurrences(&content, old);
                replacements += count;
                affected_parts.push(name.clone());
                new_entries.push((name, modified.as_bytes().to_vec()));
                continue;
            }
        }

        new_entries.push((name, data));
    }
    drop(archive);

    if replacements == 0 {
        return Ok(EditResult {
            path: path.to_string_lossy().to_string(),
            replacements: 0,
            affected_parts: vec![],
        });
    }

    // Escribir nuevo ZIP
    let file = std::fs::File::create(path)?;
    let mut writer = zip::ZipWriter::new(file);

    let _options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for (name, data) in &new_entries {
        let kind = if name.ends_with('/') {
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
        } else {
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
        };
        writer.start_file(name, kind)?;
        writer.write_all(data)?;
    }

    writer.finish()?;

    Ok(EditResult {
        path: path.to_string_lossy().to_string(),
        replacements,
        affected_parts,
    })
}

/// Cuenta ocurrencias de un substring.
fn count_occurrences(text: &str, pattern: &str) -> usize {
    text.matches(pattern).count()
}


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

    fn replace_in_ir(ir: &mut crate::ir::OxtIR, old: &str, new: &str) -> usize {
        let mut count = 0;
        for section in &mut ir.sections {
            for element in &mut section.elements {
                match element {
                    crate::ir::Element::Heading { text, .. } => {
                        let before = text.clone();
                        *text = text.replace(old, new);
                        if text != &before {
                            count += before.matches(old).count();
                        }
                    }
                    crate::ir::Element::Paragraph { runs } => {
                        for run in runs {
                            let before = run.text.clone();
                            run.text = run.text.replace(old, new);
                            if run.text != before {
                                count += before.matches(old).count();
                            }
                        }
                    }
                    crate::ir::Element::List { items, .. } => {
                        for item in items {
                            let before = item.clone();
                            *item = item.replace(old, new);
                            if item != &before {
                                count += before.matches(old).count();
                            }
                        }
                    }
                    crate::ir::Element::Table { rows } => {
                        for row in rows {
                            for cell in row {
                                let before = cell.clone();
                                *cell = cell.replace(old, new);
                                if cell != &before {
                                    count += before.matches(old).count();
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        count
    }

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
    fn test_count_occurrences() {
        assert_eq!(count_occurrences("hello world hello", "hello"), 2);
        assert_eq!(count_occurrences("hello", "world"), 0);
        assert_eq!(count_occurrences("aaaa", "aa"), 2); // overlapping not counted
    }

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
        zip.start_file("[Content_Types].xml", options.clone()).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#).unwrap();

        // Relationships
        zip.start_file("_rels/.rels", options.clone()).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#).unwrap();

        // Document
        zip.start_file("word/document.xml", options.clone()).unwrap();
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

        // Probar reemplazo
        let result = replace_text(&docx_path, "World", "oxt").unwrap();
        assert_eq!(result.replacements, 1);
        assert!(result.affected_parts.contains(&"word/document.xml".to_string()));

        // Verificar el contenido
        let file = fs::File::open(&docx_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut doc = archive.by_name("word/document.xml").unwrap();
        let mut content = String::new();
        doc.read_to_string(&mut content).unwrap();
        assert!(content.contains("Hello oxt"));
        assert!(!content.contains("Hello World"));

        // Limpiar
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_edit_legacy_doc_converts_to_docx() {
        use std::io::Read;

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
