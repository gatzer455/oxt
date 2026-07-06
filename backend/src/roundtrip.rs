//! # Roundtrip — preservation bag para edición fiel de documentos
//!
//! Lee un documento ZIP completo, parsea la parte principal a OxtIR,
//! preserva TODAS las demás partes como raw bytes, y al re-escribir
//! mergea el OxtIR modificado con las partes originales.
//!
//! Esto permite editar el IR (cambiar texto, estructura, formato) sin
//! perder estilos, imágenes, encabezados, temas, etc.

use std::io::{Read, Write};
use std::path::Path;

use crate::ir::OxtIR;
use crate::Document;

/// Error del módulo roundtrip.
#[derive(Debug, thiserror::Error)]
pub enum RoundtripError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Formato no soportado: {0}")]
    UnsupportedFormat(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, RoundtripError>;

/// Un documento abierto con preservation bag.
/// Mantiene el ZIP original en memoria y permite re-escribir solo
/// la parte que cambió (document.xml / content.xml).
pub struct RoundtripDoc {
    /// OxtIR actual (modificable por el LLM)
    pub ir: OxtIR,
    /// Formato del documento original
    pub format: crate::ir::DocumentFormat,
    /// Ruta original
    #[allow(dead_code)]
    pub path: String,
    /// Todas las partes del ZIP como raw bytes
    pub parts: Vec<(String, Vec<u8>)>,
    /// Índice de la parte principal (document.xml o content.xml)
    pub main_part_index: Option<usize>,
}

impl RoundtripDoc {
    /// Abrir un documento con preservation bag.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let path_str = path.to_string_lossy().to_string();

        // Leer el ZIP completo
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
        let fmt = crate::ir::DocumentFormat::from_path(path)
            .ok_or_else(|| RoundtripError::UnsupportedFormat(
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("(sin extensión)")
                    .to_string()
            ))?;

        // Identificar la parte principal según el formato
        let main_part = match fmt {
            crate::ir::DocumentFormat::Docx => "word/document.xml",
            crate::ir::DocumentFormat::Xlsx => "xl/sharedStrings.xml", // hay varias, la principal es variable
            crate::ir::DocumentFormat::Pptx => "ppt/slides/slide1.xml", // hay varias
            crate::ir::DocumentFormat::Odt |
            crate::ir::DocumentFormat::Ods |
            crate::ir::DocumentFormat::Odp => "content.xml",
            crate::ir::DocumentFormat::Doc |
            crate::ir::DocumentFormat::Xls |
            crate::ir::DocumentFormat::Ppt => {
                return Err(RoundtripError::UnsupportedFormat(
                    format!("{fmt}: roundtrip no soportado para legacy binary, use 'oxt edit' en su lugar")
                ));
            }
        };

        let mut main_part_index = None;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;

            // Detectar si es la parte principal por el nombre exacto
            // o por patrón (para sheets y slides)
            let is_main = if name == main_part {
                true
            } else if fmt == crate::ir::DocumentFormat::Xlsx && name.starts_with("xl/worksheets/sheet") {
                true // Tratamos sheets como partes principales
            } else if fmt == crate::ir::DocumentFormat::Pptx && name.starts_with("ppt/slides/slide") {
                true
            } else {
                false
            };

            if is_main {
                main_part_index = Some(i);
            }

            parts.push((name, data));
        }

        // Leer el documento via el reader estándar para obtener el IR
        let doc = Document::open(path)
            .map_err(|e| RoundtripError::Other(format!("Error al leer: {e}")))?;
        let ir = doc.into_ir();

        Ok(Self {
            ir,
            format: fmt,
            path: path_str,
            parts,
            main_part_index,
        })
    }

    /// Reemplazar texto en el IR y guardar con preservation.
    /// Retorna la cantidad de reemplazos realizados.
    pub fn replace_text_and_save(&mut self, old: &str, new: &str, path: impl AsRef<Path>) -> Result<usize> {
        let mut count = 0;
        for section in &mut self.ir.sections {
            for element in &mut section.elements {
                use crate::ir::Element;
                match element {
                    Element::Heading { text, .. } => {
                        let before = text.clone();
                        *text = text.replace(old, new);
                        count += (before.len() - text.len()) / old.len();
                    }
                    Element::Paragraph { runs } => {
                        for run in runs {
                            let before = run.text.clone();
                            run.text = run.text.replace(old, new);
                            count += (before.len() - run.text.len()) / old.len();
                        }
                    }
                    Element::List { items, .. } => {
                        for item in items {
                            let before = item.clone();
                            *item = item.replace(old, new);
                            count += (before.len() - item.len()) / old.len();
                        }
                    }
                    Element::Table { rows } => {
                        for row in rows {
                            for cell in row {
                                let before = cell.clone();
                                *cell = cell.replace(old, new);
                                count += (before.len() - cell.len()) / old.len();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        self.save(path)?;
        Ok(count)
    }

    /// Guardar el documento, regenerando solo la parte principal.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {

        let path = path.as_ref();
        let file = std::fs::File::create(path)?;
        let mut zip = zip::ZipWriter::new(file);

        for (i, (name, data)) in self.parts.iter().enumerate() {
            let is_main = Some(i) == self.main_part_index;

            if is_main {
                // Regenerar la parte principal desde el IR
                let opts = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated);
                zip.start_file(name, opts)?;

                let content = self.regenerate_main_part()?;
                zip.write_all(content.as_bytes())?;
            } else {
                // Preservar la parte original intacta
                let is_dir = name.ends_with('/');
                let opts = if is_dir {
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored)
                } else {
                    // Usar el mismo método de compresión que el original
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated)
                };
                zip.start_file(name, opts)?;
                zip.write_all(data)?;
            }
        }

        zip.finish()?;
        Ok(())
    }

    /// Regenerar el XML de la parte principal desde el OxtIR.
    fn regenerate_main_part(&self) -> Result<String> {
        match self.format {
            crate::ir::DocumentFormat::Docx => {
                let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<w:body>"#);
                Self::write_body_xml(&mut xml, &self.ir);
                xml.push_str("</w:body></w:document>");
                Ok(xml)
            }
            crate::ir::DocumentFormat::Odt => {
                let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content
  xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
  office:version="1.2">
  <office:body>
    <office:text>"#);
                Self::write_odf_body(&mut xml, &self.ir);
                xml.push_str("</office:text></office:body></office:document-content>");
                Ok(xml)
            }
            _ => Err(RoundtripError::UnsupportedFormat(
                format!("{}", self.format)
            )),
        }
    }

    fn write_body_xml(xml: &mut String, ir: &OxtIR) {
        use crate::ir::Element;
        for section in &ir.sections {
            for element in &section.elements {
                match element {
                    Element::Heading { level, text } => {
                        xml.push_str(&format!(r#"<w:p><w:pPr><w:pStyle w:val="Heading{level}"/></w:pPr><w:r><w:t>{}</w:t></w:r></w:p>"#,
                            Self::escape_xml(text)));
                    }
                    Element::Paragraph { runs } => {
                        xml.push_str("<w:p>");
                        for run in runs {
                            xml.push_str("<w:r>");
                            let has_fmt = run.bold.unwrap_or(false) || run.italic.unwrap_or(false)
                                || run.underline.unwrap_or(false) || run.strikethrough.unwrap_or(false)
                                || run.font_size.is_some() || run.color.is_some();
                            if has_fmt {
                                xml.push_str("<w:rPr>");
                                if run.bold.unwrap_or(false) { xml.push_str("<w:b/>"); }
                                if run.italic.unwrap_or(false) { xml.push_str("<w:i/>"); }
                                if run.underline.unwrap_or(false) { xml.push_str("<w:u/>"); }
                                if run.strikethrough.unwrap_or(false) { xml.push_str("<w:strike/>"); }
                                if let Some(sz) = run.font_size {
                                    xml.push_str(&format!(r#"<w:sz w:val="{sz}"/>"#));
                                }
                                if let Some(ref c) = run.color {
                                    xml.push_str(&format!(r#"<w:color w:val="{c}"/>"#));
                                }
                                xml.push_str("</w:rPr>");
                            }
                            xml.push_str(&format!("<w:t>{}</w:t></w:r>", Self::escape_xml(&run.text)));
                        }
                        xml.push_str("</w:p>");
                    }
                    Element::List { ordered: _, items } => {
                        xml.push_str("<w:p>");
                        for item in items {
                            xml.push_str(&format!("<w:r><w:t>• {}</w:t></w:r>", Self::escape_xml(item)));
                        }
                        xml.push_str("</w:p>");
                    }
                    Element::Table { rows } => {
                        xml.push_str(r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/></w:tblPr>"#);
                        for row in rows {
                            xml.push_str("<w:tr>");
                            for cell in row {
                                xml.push_str(&format!(r#"<w:tc><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:tc>"#, Self::escape_xml(cell)));
                            }
                            xml.push_str("</w:tr>");
                        }
                        xml.push_str("</w:tbl>");
                    }
                    _ => {}
                }
            }
        }
    }

    fn write_odf_body(xml: &mut String, ir: &OxtIR) {
        use crate::ir::Element;
        for section in &ir.sections {
            for element in &section.elements {
                match element {
                    Element::Heading { level, text } => {
                        xml.push_str(&format!(r#"<text:h text:outline-level="{level}">{}</text:h>"#, Self::escape_xml(text)));
                    }
                    Element::Paragraph { runs } => {
                        for run in runs {
                            xml.push_str(&format!("<text:p>{}</text:p>", Self::escape_xml(&run.text)));
                        }
                    }
                    Element::List { items, .. } => {
                        xml.push_str("<text:list>");
                        for item in items {
                            xml.push_str(&format!(r#"<text:list-item><text:p>{}</text:p></text:list-item>"#, Self::escape_xml(item)));
                        }
                        xml.push_str("</text:list>");
                    }
                    Element::Table { rows } => {
                        xml.push_str("<table:table>");
                        for row in rows {
                            xml.push_str("<table:table-row>");
                            for cell in row {
                                xml.push_str(&format!(r#"<table:table-cell><text:p>{}</text:p></table:table-cell>"#, Self::escape_xml(cell)));
                            }
                            xml.push_str("</table:table-row>");
                        }
                        xml.push_str("</table:table>");
                    }
                    _ => {}
                }
            }
        }
    }

    fn escape_xml(s: &str) -> String {
        s.replace('&', "&amp;")
         .replace('<', "&lt;")
         .replace('>', "&gt;")
         .replace('"', "&quot;")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;
    #[test]
    fn test_roundtrip_preserves_all_parts() {
        let src = "/tmp/test_roundtrip_real.docx";
        if !std::path::Path::new(src).exists() {
            eprintln!("Skipping: {src} not found");
            return;
        }

        // Contar partes originales
        let file = std::fs::File::open(src).unwrap();
        let mut orig = zip::ZipArchive::new(file).unwrap();
        let orig_count = orig.len();
        let orig_names: std::collections::HashSet<String> =
            (0..orig_count).map(|i| orig.by_index(i).unwrap().name().to_string()).collect();
        drop(orig);

        // Abrir con roundtrip
        let doc = super::RoundtripDoc::open(src).unwrap();
        assert_eq!(doc.parts.len(), orig_count, "mismo número de partes");

        // Guardar
        let dir = std::env::temp_dir().join("oxt_test_rt_parts");
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join("rt_out.docx");
        doc.save(&out).unwrap();

        // Verificar partes
        let file = std::fs::File::open(&out).unwrap();
        let mut saved = zip::ZipArchive::new(file).unwrap();
        let saved_count = saved.len();
        let saved_names: std::collections::HashSet<String> =
            (0..saved_count).map(|i| saved.by_index(i).unwrap().name().to_string()).collect();

        assert_eq!(saved_count, orig_count, "mismo número de partes al guardar");
        assert_eq!(saved_names, orig_names, "mismos nombres de partes");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_roundtrip_docx_preserves_structure() {
        // Crear un DOCX con formato básico
        let dir = std::env::temp_dir().join("oxt_test_rt");
        let _ = std::fs::create_dir_all(&dir);
        let docx_path = dir.join("test_rt.docx");

        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: None,
                elements: vec![
                    Element::Heading { level: 1, text: "Título".into() },
                    Element::Paragraph {
                        runs: vec![
                            Run {
                                text: "Texto en negrita".into(),
                                bold: Some(true),
                                italic: None,
                                underline: None,
                                strikethrough: None,
                                font_size: None,
                                hyperlink: None,
                                color: Some("FF0000".into()),
                            },
                        ],
                    },
                ],
            }],
        };

        crate::create::create_from_ir(&docx_path, &ir).unwrap();

        // Abrir con roundtrip
        let doc = RoundtripDoc::open(&docx_path).unwrap();
        assert_eq!(doc.format, crate::ir::DocumentFormat::Docx);

        // Verificar que las partes se preservaron
        assert!(doc.parts.iter().any(|(n, _)| n == "[Content_Types].xml"));
        assert!(doc.parts.iter().any(|(n, _)| n == "_rels/.rels"));

        // Guardar y verificar que el IR se preserva
        let out_path = dir.join("test_rt_out.docx");
        doc.save(&out_path).unwrap();

        let doc2 = crate::Document::open(&out_path).unwrap();
        let text = doc2.plain_text();
        assert!(text.contains("Título"));
        assert!(text.contains("Texto en negrita"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
