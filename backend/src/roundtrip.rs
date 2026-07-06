//! # Roundtrip — preservation bag para edición fiel de documentos
//!
//! Lee un documento ZIP completo, parsea la parte principal a OxtIR,
//! preserva TODAS las demás partes como raw bytes, y al re-escribir
//! mergea el OxtIR modificado con las partes originales.
//!
//! Esto permite editar el IR (cambiar texto, estructura, formato) sin
//! perder estilos, imágenes, encabezados, temas, etc.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

use crate::ir::{Element, OxtIR};
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
/// la parte que cambió.
#[derive(Clone)]
pub struct ZipPart {
    pub name: String,
    pub data: Vec<u8>,
    pub compression_method: zip::CompressionMethod,
}

pub struct RoundtripDoc {
    /// OxtIR actual (modificable por el LLM)
    pub ir: OxtIR,
    /// Formato del documento original
    pub format: crate::ir::DocumentFormat,
    /// Ruta original
    #[allow(dead_code)]
    pub path: String,
    /// Todas las partes del ZIP como raw bytes
    pub parts: Vec<ZipPart>,
    /// Índices de las partes que deben regenerarse (en lugar de preservarse)
    pub regenerated_indices: Vec<usize>,
}

impl RoundtripDoc {
    /// Abrir un documento con preservation bag.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let path_str = path.to_string_lossy().to_string();

        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut parts: Vec<ZipPart> = Vec::new();
        let fmt = crate::ir::DocumentFormat::from_path(path)
            .ok_or_else(|| RoundtripError::UnsupportedFormat(
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("(sin extensión)")
                    .to_string()
            ))?;

        // Rechazar legacy binary para roundtrip (se convierten en oxt_backend::edit)
        if matches!(fmt,
            crate::ir::DocumentFormat::Doc |
            crate::ir::DocumentFormat::Xls |
            crate::ir::DocumentFormat::Ppt
        ) {
            return Err(RoundtripError::UnsupportedFormat(
                format!("{fmt}: roundtrip no soportado para legacy binary, use 'oxt edit' en su lugar")
            ));
        }

        // Identificar las partes a regenerar según el formato
        let regen_patterns: &[&str] = match fmt {
            crate::ir::DocumentFormat::Docx => &["word/document.xml"],
            crate::ir::DocumentFormat::Odt => &["content.xml"],
            crate::ir::DocumentFormat::Ods => &["content.xml"],
            crate::ir::DocumentFormat::Odp => &["content.xml"],
            crate::ir::DocumentFormat::Xlsx => &[
                "xl/workbook.xml",
                "xl/sharedStrings.xml",
                "xl/worksheets/",
            ],
            crate::ir::DocumentFormat::Pptx => &[
                "ppt/presentation.xml",
                "ppt/slides/slide",
            ],
            _ => &[],
        };

        let mut regenerated_indices = Vec::new();

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            let compression_method = entry.compression();

            // Determinar si esta parte debe regenerarse
            let should_regen = regen_patterns.iter().any(|pat| name.starts_with(pat) || name == *pat);
            if should_regen {
                regenerated_indices.push(i);
            }

            parts.push(ZipPart { name, data, compression_method });
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
            regenerated_indices,
        })
    }

    /// Reemplazar texto en el IR y guardar con preservation.
    /// Retorna la cantidad de reemplazos realizados.
    pub fn replace_text_and_save(&mut self, old: &str, new: &str, path: impl AsRef<Path>) -> Result<usize> {
        let count = replace_in_ir(&mut self.ir, old, new);
        self.save(path)?;
        Ok(count)
    }

    /// Guardar el documento, regenerando solo las partes de contenido.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let file = std::fs::File::create(path)?;
        let mut zip = zip::ZipWriter::new(file);

        let regen_map = self.get_regenerated_files()?;

        for (i, part) in self.parts.iter().enumerate() {
            if self.regenerated_indices.contains(&i) {
                // Esta parte se regenera desde el IR
                if let Some(new_content) = regen_map.get(&part.name) {
                    let opts = zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated);
                    zip.start_file(&part.name, opts)?;
                    zip.write_all(new_content.as_bytes())?;
                } else if part.name.ends_with('/') {
                    zip.add_directory(&part.name, zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored))?;
                } else {
                    // Parte marcada para regenerar pero no encontrada — preservar igual
                    let opts = zip::write::SimpleFileOptions::default()
                        .compression_method(part.compression_method);
                    zip.start_file(&part.name, opts)?;
                    zip.write_all(&part.data)?;
                }
            } else if part.name.ends_with('/') {
                zip.add_directory(&part.name, zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored))?;
            } else {
                let opts = zip::write::SimpleFileOptions::default()
                    .compression_method(part.compression_method);
                zip.start_file(&part.name, opts)?;
                zip.write_all(&part.data)?;
            }
        }

        zip.finish()?;
        Ok(())
    }

    /// Generar el contenido actualizado de todas las partes que cambian.
    fn get_regenerated_files(&self) -> Result<HashMap<String, String>> {
        let mut map = HashMap::new();

        match self.format {
            crate::ir::DocumentFormat::Docx => {
                let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<w:body>"#);
                Self::write_body_xml(&mut xml, &self.ir);
                xml.push_str("</w:body></w:document>");
                map.insert("word/document.xml".to_string(), xml);
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
                Self::write_odf_text_body(&mut xml, &self.ir);
                xml.push_str("</office:text></office:body></office:document-content>");
                map.insert("content.xml".to_string(), xml);
            }

            crate::ir::DocumentFormat::Ods => {
                let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content
  xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
  office:version="1.2">
  <office:body>
    <office:spreadsheet>"#);
                for section in &self.ir.sections {
                    for element in &section.elements {
                        if let Element::Table { rows } = element {
                            xml.push_str("<table:table>");
                            for row in rows {
                                xml.push_str("<table:table-row>");
                                for cell in row {
                                    xml.push_str(&format!(
                                        r#"<table:table-cell><text:p>{}</text:p></table:table-cell>"#,
                                        Self::escape_xml(cell)
                                    ));
                                }
                                xml.push_str("</table:table-row>");
                            }
                            xml.push_str("</table:table>");
                        }
                    }
                }
                xml.push_str("</office:spreadsheet></office:body></office:document-content>");
                map.insert("content.xml".to_string(), xml);
            }

            crate::ir::DocumentFormat::Odp => {
                let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content
  xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
  xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"
  office:version="1.2">
  <office:body>
    <office:presentation>"#);
                for section in &self.ir.sections {
                    xml.push_str("<draw:page>");
                    if let Some(ref title) = section.title {
                        xml.push_str(r#"<draw:frame><draw:text-box>"#);
                        xml.push_str(&format!(r#"<text:p>{}</text:p>"#, Self::escape_xml(title)));
                        xml.push_str(r#"</draw:text-box></draw:frame>"#);
                    }
                    for element in &section.elements {
                        match element {
                            Element::Heading { text, .. } => {
                                xml.push_str(&format!(r#"<draw:frame><draw:text-box><text:p>{}</text:p></draw:text-box></draw:frame>"#, Self::escape_xml(text)));
                            }
                            Element::Paragraph { runs } => {
                                let text: String = runs.iter().map(|r| r.text.as_str()).collect();
                                xml.push_str(&format!(r#"<draw:frame><draw:text-box><text:p>{}</text:p></draw:text-box></draw:frame>"#, Self::escape_xml(&text)));
                            }
                            Element::List { items, .. } => {
                                for item in items {
                                    xml.push_str(&format!(r#"<draw:frame><draw:text-box><text:p>• {}</text:p></draw:text-box></draw:frame>"#, Self::escape_xml(item)));
                                }
                            }
                            _ => {}
                        }
                    }
                    xml.push_str("</draw:page>");
                }
                xml.push_str("</office:presentation></office:body></office:document-content>");
                map.insert("content.xml".to_string(), xml);
            }

            crate::ir::DocumentFormat::Xlsx => {
                // Regenerar xl/workbook.xml
                let mut wb = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
          xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets>"#);
                for (i, section) in self.ir.sections.iter().enumerate() {
                    let default_name = format!("Sheet{}", i + 1);
                    let name = section.title.as_deref().unwrap_or(&default_name);
                    wb.push_str(&format!(
                        r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#,
                        Self::escape_xml(name),
                        i + 1,
                        i,
                    ));
                }
                wb.push_str("</sheets></workbook>");
                map.insert("xl/workbook.xml".to_string(), wb);

                // Regenerar xl/sharedStrings.xml
                let mut all_strings: Vec<String> = Vec::new();
                let mut string_index: HashMap<String, u32> = HashMap::new();

                // Recolectar strings de todas las tablas
                for section in &self.ir.sections {
                    for element in &section.elements {
                        if let Element::Table { rows } = element {
                            for row in rows {
                                for cell in row {
                                    if !string_index.contains_key(cell) {
                                        string_index.insert(cell.clone(), all_strings.len() as u32);
                                        all_strings.push(cell.clone());
                                    }
                                }
                            }
                        }
                    }
                }

                let mut ss = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
     count="UNIQ" uniqueCount="UNIQ">"#);
                for s in &all_strings {
                    ss.push_str(&format!("<si><t>{}</t></si>", Self::escape_xml(s)));
                }
                ss.push_str("</sst>");
                // Reemplazar UNIQ con el count real
                let uniq = all_strings.len();
                let ss = ss.replace("UNIQ", &uniq.to_string());
                map.insert("xl/sharedStrings.xml".to_string(), ss);

                // Regenerar xl/worksheets/sheetN.xml para cada sección
                for (i, section) in self.ir.sections.iter().enumerate() {
                    // Buscar la primera tabla en esta sección
                    let mut rows_data: Vec<Vec<String>> = Vec::new();
                    for element in &section.elements {
                        if let Element::Table { rows } = element {
                            rows_data = rows.clone();
                            break;
                        }
                    }

                    let mut ws = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>"#);
                    for (row_idx, row) in rows_data.iter().enumerate() {
                        ws.push_str(&format!("<row r=\"{}\">", row_idx + 1));
                        for (col_idx, cell) in row.iter().enumerate() {
                            let col_ref = col_to_excel(col_idx);
                            // Buscar el índice del string
                            if let Some(si) = string_index.get(cell) {
                                ws.push_str(&format!(
                                    r#"<c r="{}{}" t="s"><v>{}</v></c>"#,
                                    col_ref, row_idx + 1, si
                                ));
                            }
                        }
                        ws.push_str("</row>");
                    }
                    ws.push_str("</sheetData></worksheet>");

                    map.insert(format!("xl/worksheets/sheet{}.xml", i + 1), ws);
                }
            }

            crate::ir::DocumentFormat::Pptx => {
                // Regenerar ppt/presentation.xml
                let mut pres = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldIdLst>"#);
                for (i, _) in self.ir.sections.iter().enumerate() {
                    pres.push_str(&format!(
                        r#"<p:sldId id="{}" r:id="rId{}"/>"#,
                        i + 1, i,
                    ));
                }
                pres.push_str("</p:sldIdLst><p:sldSz cx=\"9144000\" cy=\"6858000\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>");
                map.insert("ppt/presentation.xml".to_string(), pres);

                // Regenerar ppt/slides/slideN.xml para cada sección
                for (i, section) in self.ir.sections.iter().enumerate() {
                    let mut slide = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr>
        <p:cNvPr id="1" name=""/>
        <p:cNvGrpSpPr/>
        <p:nvPr/>
      </p:nvGrpSpPr>
      <p:grpSpPr/>"#);

                    let mut shape_id: u32 = 2;

                    // Título de la diapositiva
                    if let Some(ref title) = section.title {
                        slide.push_str(&format!(
                            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Title {id}"/><p:cNvSpPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="457200" y="274320"/><a:ext cx="8229600" cy="822960"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="es-ES" sz="2400" b="1"/><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>"#,
                            Self::escape_xml(title),
                            id = shape_id,
                        ));
                        shape_id += 1;
                    }

                    // Elementos de contenido
                    for element in &section.elements {
                        match element {
                            Element::Heading { text, .. } => {
                                slide.push_str(&format!(
                                    r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Heading {id}"/><p:cNvSpPr/></p:nvSpPr><p:spPr><a:xfrm xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:off x="457200" y="914400"/><a:ext cx="8229600" cy="822960"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="es-ES" sz="2000" b="1"/><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>"#,
                                    Self::escape_xml(text),
                                    id = shape_id,
                                ));
                                shape_id += 1;
                            }
                            Element::Paragraph { runs } => {
                                let text: String = runs.iter().map(|r| r.text.as_str()).collect();
                                slide.push_str(&format!(
                                    r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Paragraph {id}"/><p:cNvSpPr/></p:nvSpPr><p:spPr><a:xfrm xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:off x="457200" y="1828800"/><a:ext cx="8229600" cy="548640"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="es-ES" sz="1600"/><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>"#,
                                    Self::escape_xml(&text),
                                    id = shape_id,
                                ));
                                shape_id += 1;
                            }
                            Element::List { items, .. } => {
                                for item in items {
                                    slide.push_str(&format!(
                                        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="ListItem {id}"/><p:cNvSpPr/></p:nvSpPr><p:spPr><a:xfrm xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:off x="457200" y="1828800"/><a:ext cx="8229600" cy="365760"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="es-ES" sz="1600"/><a:t>• {}</a:t></a:r></a:p></p:txBody></p:sp>"#,
                                        Self::escape_xml(item),
                                        id = shape_id,
                                    ));
                                    shape_id += 1;
                                }
                            }
                            Element::Table { rows } => {
                                slide.push_str(&format!(
                                    r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Table {id}"/><p:cNvSpPr/></p:nvSpPr><p:spPr><a:xfrm xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:off x="457200" y="1828800"/><a:ext cx="8229600" cy="2743200"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/>"#,
                                    id = shape_id,
                                ));
                                shape_id += 1;
                                slide.push_str(r#"<a:tbl><a:tblPr/><a:tblGrid>"#);
                                if let Some(first) = rows.first() {
                                    for _ in first {
                                        slide.push_str(r#"<a:gridCol w="2743200"/>"#);
                                    }
                                }
                                slide.push_str(r#"</a:tblGrid>"#);
                                for row in rows {
                                    slide.push_str("<a:tr>");
                                    for cell in row {
                                        slide.push_str(&format!(
                                            r#"<a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>{}</a:t></a:r></a:p></a:txBody></a:tc>"#,
                                            Self::escape_xml(cell)
                                        ));
                                    }
                                    slide.push_str("</a:tr>");
                                }
                                slide.push_str(r#"</a:tbl></p:txBody></p:sp>"#);
                            }
                            _ => {}
                        }
                    }

                    slide.push_str("</p:spTree></p:cSld></p:sld>");
                    map.insert(format!("ppt/slides/slide{}.xml", i), slide);
                }
            }

            _ => {
                return Err(RoundtripError::UnsupportedFormat(format!("{}", self.format)));
            }
        }

        Ok(map)
    }

    fn write_body_xml(xml: &mut String, ir: &OxtIR) {
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

    fn write_odf_text_body(xml: &mut String, ir: &OxtIR) {
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

/// Reemplazar texto en el OxtIR (recorre sections → elements → runs)
fn replace_in_ir(ir: &mut OxtIR, old: &str, new: &str) -> usize {
    let mut count = 0;
    for section in &mut ir.sections {
        for element in &mut section.elements {
            match element {
                Element::Heading { text, .. } => {
                    let before = text.clone();
                    *text = text.replace(old, new);
                    count += before.matches(old).count();
                }
                Element::Paragraph { runs } => {
                    for run in runs {
                        let before = run.text.clone();
                        run.text = run.text.replace(old, new);
                        count += before.matches(old).count();
                    }
                }
                Element::List { items, .. } => {
                    for item in items {
                        let before = item.clone();
                        *item = item.replace(old, new);
                        count += before.matches(old).count();
                    }
                }
                Element::Table { rows } => {
                    for row in rows {
                        for cell in row {
                            let before = cell.clone();
                            *cell = cell.replace(old, new);
                            count += before.matches(old).count();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    count
}

/// Convertir número de columna (0-indexed) a referencia Excel (A, B, ..., Z, AA, AB...)
fn col_to_excel(mut col: usize) -> String {
    let mut result = String::new();
    loop {
        let rem = col % 26;
        result.insert(0, (b'A' + rem as u8) as char);
        col /= 26;
        if col == 0 { break; }
        col -= 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    #[test]
    fn test_roundtrip_xlsx_edit_workflow() {
        let dir = std::env::temp_dir().join("oxt_test_rt_xlsx");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("test.xlsx");

        // Crear XLSX
        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: Some("Sheet1".into()),
                elements: vec![Element::Table {
                    rows: vec![
                        vec!["Nombre".into(), "Edad".into()],
                        vec!["Ana".into(), "28".into()],
                    ],
                }],
            }],
        };
        crate::create::create_from_ir(&p, &ir).unwrap();

        // Abrir y editar con roundtrip
        let mut doc = RoundtripDoc::open(&p).unwrap();
        assert_eq!(doc.format, crate::ir::DocumentFormat::Xlsx);
        assert!(doc.regenerated_indices.len() >= 3, "debe regenerar al menos 3 archivos");

        let count = doc.replace_text_and_save("Ana", "María", &p).unwrap();
        assert_eq!(count, 1, "debe reemplazar 1 ocurrencia");

        // Verificar que el cambio persiste
        let doc2 = crate::Document::open(&p).unwrap();
        let text = doc2.plain_text();
        assert!(text.contains("María"), "debe tener María: {text:?}");
        assert!(!text.contains("Ana"), "no debe tener Ana: {text:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_roundtrip_pptx_edit_workflow() {
        let dir = std::env::temp_dir().join("oxt_test_rt_pptx");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("test.pptx");

        // Crear PPTX
        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![
                Section {
                    title: Some("Slide 1".into()),
                    elements: vec![
                        Element::Heading { level: 1, text: "Título".into() },
                        Element::Paragraph { runs: vec![Run::plain("Contenido")] },
                    ],
                },
                Section {
                    title: Some("Slide 2".into()),
                    elements: vec![
                        Element::List { ordered: false, items: vec!["Uno".into(), "Dos".into()] },
                    ],
                },
            ],
        };
        crate::create::create_from_ir(&p, &ir).unwrap();

        // Abrir y editar con roundtrip
        let mut doc = RoundtripDoc::open(&p).unwrap();
        assert_eq!(doc.format, crate::ir::DocumentFormat::Pptx);

        let count = doc.replace_text_and_save("Título", "Editado", &p).unwrap();
        assert_eq!(count, 1);

        let doc2 = crate::Document::open(&p).unwrap();
        let text = doc2.plain_text();
        assert!(text.contains("Editado"), "debe tener Editado: {text:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_roundtrip_ods_edit_workflow() {
        let dir = std::env::temp_dir().join("oxt_test_rt_ods");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("test.ods");

        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: None,
                elements: vec![Element::Table {
                    rows: vec![
                        vec!["A".into(), "B".into()],
                        vec!["1".into(), "2".into()],
                    ],
                }],
            }],
        };
        crate::create::create_from_ir(&p, &ir).unwrap();

        let mut doc = RoundtripDoc::open(&p).unwrap();
        let count = doc.replace_text_and_save("2", "999", &p).unwrap();
        assert_eq!(count, 1);

        let doc2 = crate::Document::open(&p).unwrap();
        let text = doc2.plain_text();
        assert!(text.contains("999"), "debe tener 999: {text:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_roundtrip_odp_edit_workflow() {
        let dir = std::env::temp_dir().join("oxt_test_rt_odp");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("test.odp");

        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: Some("Slide".into()),
                elements: vec![Element::Paragraph { runs: vec![Run::plain("Hola ODP")] }],
            }],
        };
        crate::create::create_from_ir(&p, &ir).unwrap();

        let mut doc = RoundtripDoc::open(&p).unwrap();
        let count = doc.replace_text_and_save("Hola", "Editado", &p).unwrap();
        assert_eq!(count, 1);

        let doc2 = crate::Document::open(&p).unwrap();
        let text = doc2.plain_text();
        assert!(text.contains("Editado"), "debe tener Editado: {text:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_roundtrip_preserves_all_parts() {
        let src = "/tmp/test_roundtrip_real.docx";
        if !std::path::Path::new(src).exists() {
            eprintln!("Skipping: {src} not found");
            return;
        }

        let file = std::fs::File::open(src).unwrap();
        let mut orig = zip::ZipArchive::new(file).unwrap();
        let orig_count = orig.len();
        let orig_names: std::collections::HashSet<String> =
            (0..orig_count).map(|i| orig.by_index(i).unwrap().name().to_string()).collect();
        drop(orig);

        let doc = super::RoundtripDoc::open(src).unwrap();
        assert_eq!(doc.parts.len(), orig_count, "mismo número de partes");

        let dir = std::env::temp_dir().join("oxt_test_rt_parts");
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join("rt_out.docx");
        doc.save(&out).unwrap();

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
                        runs: vec![Run {
                            text: "Texto en negrita".into(),
                            bold: Some(true),
                            italic: None,
                            underline: None,
                            strikethrough: None,
                            font_size: None,
                            hyperlink: None,
                            color: Some("FF0000".into()),
                        }],
                    },
                ],
            }],
        };

        crate::create::create_from_ir(&docx_path, &ir).unwrap();

        let doc = RoundtripDoc::open(&docx_path).unwrap();
        assert_eq!(doc.format, crate::ir::DocumentFormat::Docx);
        assert!(doc.parts.iter().any(|p| p.name == "[Content_Types].xml"));
        assert!(doc.parts.iter().any(|p| p.name == "_rels/.rels"));

        let out_path = dir.join("test_rt_out.docx");
        doc.save(&out_path).unwrap();

        let doc2 = crate::Document::open(&out_path).unwrap();
        let text = doc2.plain_text();
        assert!(text.contains("Título"));
        assert!(text.contains("Texto en negrita"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_roundtrip_replace_text_workflow() {
        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: None,
                elements: vec![
                    Element::Paragraph { runs: vec![Run::plain("Hello World from Roundtrip")] },
                ],
            }],
        };

        let dir = std::env::temp_dir().join("oxt_test_rt_replace");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("test_rt.docx");
        crate::create::create_from_ir(&p, &ir).unwrap();

        let mut doc = super::RoundtripDoc::open(&p).unwrap();
        doc.replace_text_and_save("World", "oxt", &p).unwrap();

        let doc2 = crate::Document::open(&p).unwrap();
        let text = doc2.plain_text();
        assert!(text.contains("Hello oxt"), "debe tener reemplazo: {text:?}");
        assert!(!text.contains("World"), "no debe tener original");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
