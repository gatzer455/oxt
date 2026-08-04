#![allow(unused_assignments, unused_variables)]
//! # DOCX Reader
//!
//! Parsea un archivo .docx → OxtIR.
//!
//! Un DOCX es un ZIP con:
//!   - word/document.xml   → cuerpo del documento
//!   - word/styles.xml      → estilos (para detectar headings)
//!   - word/numbering.xml   → numeración de listas
//!   - word/media/          → imágenes
//!   - word/_rels/          → relaciones
//!
//! El XML usa el namespace `w` (WordprocessingML):
//!   w:p  → párrafo
//!   w:r  → run (texto con formato)
//!   w:t  → texto
//!   w:tbl → tabla

use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use std::collections::HashMap;

use crate::ir::*;
use crate::opc::OpcPackage;
/// Un estilo de párrafo (de styles.xml).
struct StyleEntry {
    style_id: String,
    based_on: Option<String>,
    #[allow(dead_code)]
    style_type: String,
    heading_level: Option<u8>,
}


/// Error del parser DOCX.
#[derive(Debug, thiserror::Error)]
pub enum DocxError {
    #[error("OPC error: {0}")]
    Opc(#[from] crate::opc::OpcError),

    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("Part not found: {0}")]
    PartNotFound(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, DocxError>;

/// DOCX parseado.
pub struct DocxReader {
    ir: OxtIR,
}

impl DocxReader {
    /// Abrir y parsear un .docx.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let mut pkg = OpcPackage::open(path.as_ref())?;
        Self::parse(&mut pkg)
    }

    /// Parsear desde un paquete OPC ya abierto.
    pub fn parse(pkg: &mut OpcPackage<std::fs::File>) -> Result<Self> {
        let styles = Self::parse_styles(pkg)?;

        // Cargar numbering (para listas)
        let numbering = Self::parse_numbering(pkg)?;

        // Parsear document.xml
        let body_xml = pkg.read_string("word/document.xml")?;
        let rels = pkg.part_rels("word/document.xml")?;
        let rel_map: HashMap<String, String> = rels
            .iter()
            .map(|r| (r.id.clone(), r.target.clone()))
            .collect();
        let elements = Self::parse_body(pkg, &body_xml, &styles, &numbering, &rel_map)?;

        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: None,
                elements,
            }],
        };

        Ok(Self { ir })
    }

    /// Consumir y devolver el IR.
    pub fn into_ir(self) -> OxtIR {
        self.ir
    }

    // ── Styles parser ──────────────────────────────────────────────────────


    fn parse_styles(pkg: &mut OpcPackage<std::fs::File>) -> Result<HashMap<String, StyleEntry>> {
        let xml = match pkg.read_string("word/styles.xml") {
            Ok(x) => x,
            Err(_) => return Ok(HashMap::new()),
        };

        let mut reader = XmlReader::from_str(&xml);
        reader.config_mut().expand_empty_elements = true;
        let mut buf = Vec::new();

        let mut styles = HashMap::new();
        let mut current_style: Option<StyleEntry> = None;
        let mut in_based_on = false;
        let mut in_name = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    if name == b"w:style" || name == b"style" {
                        let style_id = e.try_get_attribute("w:styleId").ok().flatten()
                            .or_else(|| e.try_get_attribute("styleId").ok().flatten())
                            .map(|a| String::from_utf8_lossy(&a.value).to_string());
                        let style_type = e.try_get_attribute("w:type").ok().flatten()
                            .or_else(|| e.try_get_attribute("type").ok().flatten())
                            .map(|a| String::from_utf8_lossy(&a.value).to_string())
                            .unwrap_or_default();
                        if let Some(id) = style_id {
                            current_style = Some(StyleEntry {
                                style_id: id,
                                based_on: None,
                                style_type,
                                heading_level: None,
                            });
                        }
                    } else if current_style.is_some() {
                        let inner = e.name().as_ref().to_vec();
                        if inner == b"w:basedOn" || inner == b"basedOn" {
                            in_based_on = true;
                        } else if inner == b"w:name" || inner == b"name" {
                            in_name = true;
                        }
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    if current_style.is_some() {
                        let inner = e.name().as_ref().to_vec();
                        // <w:basedOn w:val="Normal"/>
                        if inner == b"w:basedOn" || inner == b"basedOn" {
                            if let Some(attr) = e.try_get_attribute("w:val").ok().flatten()
                                .or_else(|| e.try_get_attribute("val").ok().flatten())
                            {
                                if let Some(ref mut cs) = current_style {
                                    cs.based_on = Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                        }
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if let Some(ref mut cs) = current_style {
                        let text = e.unescape()?;
                        if in_based_on {
                            cs.based_on = Some(text.to_string());
                            in_based_on = false;
                        }
                        if in_name {
                            // Detectar si es heading por el nombre del estilo
                            let lower = text.to_lowercase();
                            if lower.starts_with("heading") || lower.starts_with("encabezado") || lower.starts_with("título") {
                                if let Some(n) = text.chars().last().and_then(|c| c.to_digit(10)) {
                                    if (1..=6).contains(&n) {
                                        cs.heading_level = Some(n as u8);
                                    }
                                }
                            }
                            in_name = false;
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    if name == b"w:style" || name == b"style" {
                        if let Some(entry) = current_style.take() {
                            styles.insert(entry.style_id.clone(), entry);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(DocxError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        // Resolver herencia de estilos para detectar headings
        // Si un estilo hereda de Heading1, también es heading
        let keys: Vec<String> = styles.keys().cloned().collect();
        for key in &keys {
            let level = resolve_heading_level(&styles, key);
            if let Some(level) = level {
                if let Some(entry) = styles.get_mut(key) {
                    entry.heading_level = Some(level);
                }
            }
        }

        Ok(styles)
    }

    // ── Numbering parser ───────────────────────────────────────────────────

    fn parse_numbering(pkg: &mut OpcPackage<std::fs::File>) -> Result<HashMap<u32, bool>> {
        let xml = match pkg.read_string("word/numbering.xml") {
            Ok(x) => x,
            Err(_) => return Ok(HashMap::new()),
        };

        let mut reader = XmlReader::from_str(&xml);
        reader.config_mut().expand_empty_elements = true;
        let mut buf = Vec::new();

        // numId → ordered (true = numerada, false = bullets)
        let num_map = HashMap::new();
        let mut current_num_id: Option<u32> = None;
        let mut current_abstract_num_id: Option<u32> = None;
        let mut abstract_num_map: HashMap<u32, bool> = HashMap::new();
        let mut in_num_fmt = false;
        let mut in_abstract_num = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    if name == b"w:num" || name == b"num" {
                        current_num_id = e.try_get_attribute("w:numId").ok().flatten()
                            .or_else(|| e.try_get_attribute("numId").ok().flatten())
                            .and_then(|a| {
                                let s = String::from_utf8_lossy(&a.value).to_string();
                                s.parse::<u32>().ok()
                            });
                    } else if name == b"w:abstractNum" || name == b"abstractNum" {
                        in_abstract_num = true;
                        current_abstract_num_id = e.try_get_attribute("w:abstractNumId").ok().flatten()
                            .or_else(|| e.try_get_attribute("abstractNumId").ok().flatten())
                            .and_then(|a| {
                                let s = String::from_utf8_lossy(&a.value).to_string();
                                s.parse::<u32>().ok()
                            });
                    } else if name == b"w:numFmt" || name == b"numFmt" {
                        in_num_fmt = true;
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    if (name == b"w:numFmt" || name == b"numFmt") && current_abstract_num_id.is_some() {
                        if let Some(attr) = e.try_get_attribute("w:val").ok().flatten()
                            .or_else(|| e.try_get_attribute("val").ok().flatten())
                        {
                            let val = String::from_utf8_lossy(&attr.value);
                            let ordered = val != "bullet";
                            if let Some(id) = current_abstract_num_id {
                                abstract_num_map.insert(id, ordered);
                            }
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    if name == b"w:num" || name == b"num" {
                        current_num_id = None;
                    } else if name == b"w:abstractNum" || name == b"abstractNum" {
                        in_abstract_num = false;
                        current_abstract_num_id = None;
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(DocxError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        // Ahora resolver numId → abstractNumId (simplificado)
        // Por ahora devolvemos vacío; la detección de listas se mejora después
        Ok(num_map)
    }

    // ── Body parser ────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn parse_body(
        pkg: &mut OpcPackage<std::fs::File>,
        xml: &str,
        styles: &HashMap<String, StyleEntry>,
        _numbering: &HashMap<u32, bool>,
        rel_map: &HashMap<String, String>,
    ) -> Result<Vec<Element>> {
        let mut reader = XmlReader::from_str(xml);
        reader.config_mut().expand_empty_elements = true;
        reader.config_mut().trim_text(false);
        let mut buf = Vec::new();

        let mut elements = Vec::new();
        let mut in_paragraph = false;
        let mut in_run = false;
        let mut in_text = false;
        let mut in_table = false;
        let mut in_table_row = false;
        let mut in_table_cell = false;
        let mut in_hyperlink = false;
        let mut in_break = false;
        let mut in_pstyle = false;
        let mut in_rpr = false;
        let mut in_drawing = false;
        let mut pending_image: Option<(String, Option<String>)> = None;
        let mut pending_alt: Option<String> = None;

        let mut current_runs: Vec<Run> = Vec::new();
        let mut current_run: Option<Run> = None;
        let mut current_paragraph_style: Option<String> = None;
        let mut link_id: Option<String> = None;

        let mut table_rows: Vec<Vec<String>> = Vec::new();
        let mut table_row: Vec<String> = Vec::new();
        let mut cell_text = String::new();

        let list_items: Vec<String> = Vec::new();
        let in_list = false;
        let list_ordered = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name_data = e.name().as_ref().to_owned();
                    let tag = local_name(&name_data);
                    match tag {
                        "p" => {
                            if in_table_cell {
                                // párrafo dentro de celda — lo ignoramos,
                                // ya capturamos texto en la celda
                            } else if !in_table {
                                in_paragraph = true;
                                current_runs = Vec::new();
                                current_paragraph_style = None;
                            }
                        }
                        "pPr" if in_paragraph => {}
                        "pStyle" if in_paragraph => in_pstyle = true,
                        "r" if in_paragraph => {
                            in_run = true;
                            current_run = Some(Run::plain(""));
                            link_id = None;
                        }
                        "rPr" if in_run => in_rpr = true,
                        "t" if in_run => in_text = true,
                        "br" if in_run => in_break = true,
                        "hyperlink" if in_paragraph => {
                            in_hyperlink = true;
                            link_id = e.try_get_attribute("r:id").ok().flatten()
                                .or_else(|| e.try_get_attribute("id").ok().flatten())
                                .map(|a| String::from_utf8_lossy(&a.value).to_string());
                        }
                        "drawing" if in_run => in_drawing = true,
                        "docPr" if in_drawing => {
                            // Nombre/descripción de la imagen (alt text)
                            let alt = e.try_get_attribute("descr").ok().flatten()
                                .or_else(|| e.try_get_attribute("name").ok().flatten())
                                .map(|a| String::from_utf8_lossy(&a.value).to_string());
                            if let Some(alt) = alt {
                                pending_alt = Some(alt);
                            }
                        }
                        "blip" if in_drawing => {
                            let rid = e.try_get_attribute("r:embed").ok().flatten()
                                .or_else(|| e.try_get_attribute("embed").ok().flatten())
                                .map(|a| String::from_utf8_lossy(&a.value).to_string());
                            if let Some(rid) = rid {
                                pending_image = Some((rid, pending_alt.take()));
                            }
                        }
                        "b" if in_rpr => {
                            if let Some(ref mut run) = current_run {
                                run.bold = Some(true);
                            }
                        }
                        "i" if in_rpr => {
                            if let Some(ref mut run) = current_run {
                                run.italic = Some(true);
                            }
                        }
                        "u" if in_rpr => {
                            if let Some(ref mut run) = current_run {
                                run.underline = Some(true);
                            }
                        }
                        "strike" if in_rpr => {
                            if let Some(ref mut run) = current_run {
                                run.strikethrough = Some(true);
                            }
                        }
                        "dstrike" if in_rpr => {
                            // w:strike o w:dstrike
                            if let Some(ref mut run) = current_run {
                                run.strikethrough = Some(true);
                            }
                        }
                        "sz" if in_rpr => {
                            // w:sz w:val="24" → font_size en half-points
                            if let Some(ref mut run) = current_run {
                                if let Some(attr) = e.try_get_attribute("w:val").ok().flatten() {
                                    if let Ok(v) = std::str::from_utf8(&attr.value) {
                                        if let Ok(n) = v.parse::<f32>() {
                                            run.font_size = Some(n);
                                        }
                                    }
                                } else if let Some(attr) = e.try_get_attribute("val").ok().flatten() {
                                    if let Ok(v) = std::str::from_utf8(&attr.value) {
                                        if let Ok(n) = v.parse::<f32>() {
                                            run.font_size = Some(n);
                                        }
                                    }
                                }
                            }
                        }
                        "color" if in_rpr => {
                            // w:color w:val="FF0000"
                            if let Some(ref mut run) = current_run {
                                if let Some(attr) = e.try_get_attribute("w:val").ok().flatten() {
                                    if let Ok(v) = std::str::from_utf8(&attr.value) {
                                        run.color = Some(v.to_string());
                                    }
                                } else if let Some(attr) = e.try_get_attribute("val").ok().flatten() {
                                    if let Ok(v) = std::str::from_utf8(&attr.value) {
                                        run.color = Some(v.to_string());
                                    }
                                }
                            }
                        }
                        "tbl" if !in_table => {
                            in_table = true;
                            table_rows = Vec::new();
                        }
                        "tr" if in_table => {
                            in_table_row = true;
                            table_row = Vec::new();
                        }
                        "tc" if in_table_row => {
                            in_table_cell = true;
                            cell_text = String::new();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let name_data = e.name().as_ref().to_owned();
                    let tag = local_name(&name_data);
                    match tag {
                        "br" if in_run => in_break = true,
                        "blip" if in_drawing => {
                            let rid = e.try_get_attribute("r:embed").ok().flatten()
                                .or_else(|| e.try_get_attribute("embed").ok().flatten())
                                .map(|a| String::from_utf8_lossy(&a.value).to_string());
                            if let Some(rid) = rid {
                                pending_image = Some((rid, pending_alt.take()));
                            }
                        }
                        "b" if in_rpr => {
                            if let Some(ref mut run) = current_run {
                                run.bold = Some(true);
                            }
                        }
                        "i" if in_rpr => {
                            if let Some(ref mut run) = current_run {
                                run.italic = Some(true);
                            }
                        }
                        "u" if in_rpr => {
                            if let Some(ref mut run) = current_run {
                                run.underline = Some(true);
                            }
                        }
                        "strike" if in_rpr => {
                            if let Some(ref mut run) = current_run {
                                run.strikethrough = Some(true);
                            }
                        }
                        "dstrike" if in_rpr => {
                            if let Some(ref mut run) = current_run {
                                run.strikethrough = Some(true);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape()?;
                    if in_text {
                        if let Some(ref mut run) = current_run {
                            run.text.push_str(&text);
                        }
                    } else if in_pstyle {
                        current_paragraph_style = Some(text.to_string());
                        in_pstyle = false;
                    } else if in_table_cell {
                        cell_text.push_str(&text);
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name_data = e.name().as_ref().to_owned();
                    let tag = local_name(&name_data);
                    match tag {
                        "pStyle" => in_pstyle = false,
                        "pPr" => {}
                        "p" => {
                            if in_paragraph {
                                in_paragraph = false;

                                // Detectar si es heading por el estilo
                                if let Some(ref style_id) = current_paragraph_style {
                                    if let Some(entry) = styles.get(style_id) {
                                        if let Some(level) = entry.heading_level {
                                            let text = current_runs.iter()
                                                .map(|r| r.text.clone())
                                                .collect::<Vec<_>>()
                                                .join("");
                                            elements.push(Element::Heading { level, text });
                                            current_runs = Vec::new();
                                        }
                                    }
                                }

                                // Si no es heading, es párrafo normal
                                if !current_runs.is_empty() {
                                    elements.push(Element::Paragraph { runs: current_runs.clone() });
                                    current_runs = Vec::new();
                                }

                                // Imagen dentro del párrafo (w:drawing → r:embed)
                                if let Some((rid, alt)) = pending_image.take() {
                                    if let Some(target) = rel_map.get(&rid) {
                                        let full = if target.starts_with('/') {
                                            target.trim_start_matches('/').to_string()
                                        } else {
                                            format!("word/{target}")
                                        };
                                        if let Ok(bytes) = pkg.read_bytes(&full) {
                                            use base64::Engine;
                                            let data = base64::engine::general_purpose::STANDARD
                                                .encode(&bytes);
                                            let filename = full
                                                .rsplit('/')
                                                .next()
                                                .unwrap_or("image")
                                                .to_string();
                                            elements.push(Element::Image {
                                                filename,
                                                data,
                                                alt_text: alt,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        "r" => {
                            if let Some(run) = current_run.take() {
                                if in_break {
                                    // Salto de línea lo tratamos como texto
                                    current_runs.push(Run::plain("\n"));
                                    in_break = false;
                                }
                                if !run.text.is_empty() || run.hyperlink.is_some() {
                                    current_runs.push(run);
                                }
                            }
                            in_run = false;
                            in_break = false;
                        }
                        "rPr" => in_rpr = false,
                        "t" => in_text = false,
                        "drawing" => in_drawing = false,
                        "hyperlink" => in_hyperlink = false,
                        "tbl" => {
                            if !table_rows.is_empty() {
                                elements.push(Element::Table { rows: table_rows.clone() });
                            }
                            in_table = false;
                        }
                        "tr" => {
                            if in_table_row && !table_row.is_empty() {
                                table_rows.push(table_row.clone());
                            }
                            in_table_row = false;
                        }
                        "tc"
                            if in_table_cell => {
                                table_row.push(cell_text.trim().to_string());
                                in_table_cell = false;
                            }
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(DocxError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(elements)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Obtener el nombre local de un tag (sin namespace).
fn local_name(name: &[u8]) -> &str {
    let name = std::str::from_utf8(name).unwrap_or("");
    if let Some(pos) = name.find(':') {
        &name[pos + 1..]
    } else {
        name
    }
}

/// Resolver el nivel de heading por herencia de estilos.
fn resolve_heading_level(
    styles: &HashMap<String, StyleEntry>,
    style_id: &str,
) -> Option<u8> {
    let entry = styles.get(style_id)?;

    // Ya tiene heading level directo
    if let Some(level) = entry.heading_level {
        return Some(level);
    }

    // Buscar por herencia (máximo 5 niveles para evitar ciclos)
    let mut visited = std::collections::HashSet::new();
    let mut current = entry.based_on.as_deref();
    for _ in 0..5 {
        let parent_id = current?;
        if !visited.insert(parent_id) {
            return None; // ciclo detectado
        }
        let parent = styles.get(parent_id)?;
        if let Some(level) = parent.heading_level {
            return Some(level);
        }
        current = parent.based_on.as_deref();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_name() {
        assert_eq!(local_name(b"w:p"), "p");
        assert_eq!(local_name(b"w:r"), "r");
        assert_eq!(local_name(b"w:t"), "t");
        assert_eq!(local_name(b"body"), "body");
    }

    #[test]
    fn test_resolve_heading_level_direct() {
        let mut styles = HashMap::new();
        styles.insert("Heading1".into(), StyleEntry {
            style_id: "Heading1".into(),
            based_on: None,
            style_type: "paragraph".into(),
            heading_level: Some(1),
        });
        assert_eq!(resolve_heading_level(&styles, "Heading1"), Some(1));
    }

    #[test]
    fn test_resolve_heading_level_inherited() {
        let mut styles = HashMap::new();
        styles.insert("Heading1".into(), StyleEntry {
            style_id: "Heading1".into(),
            based_on: None,
            style_type: "paragraph".into(),
            heading_level: Some(1),
        });
        styles.insert("Titulo1".into(), StyleEntry {
            style_id: "Titulo1".into(),
            based_on: Some("Heading1".into()),
            style_type: "paragraph".into(),
            heading_level: None,
        });
        assert_eq!(resolve_heading_level(&styles, "Titulo1"), Some(1));
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use base64::Engine;
    use std::io::Write;

    fn write_docx_with_image(path: &std::path::Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        let png = [0x89u8, 0x50, 0x4E, 0x47];

        zip.start_file("[Content_Types].xml", opts).unwrap();
        zip.write_all(br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#).unwrap();
        zip.start_file("_rels/.rels", opts).unwrap();
        zip.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#).unwrap();
        zip.start_file("word/_rels/document.xml.rels", opts).unwrap();
        zip.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/foto1.png"/></Relationships>"#).unwrap();
        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p><w:r><w:t>Texto antes</w:t></w:r></w:p><w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Foto 1" descr="una foto"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId5"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p><w:p><w:r><w:t>Texto despues</w:t></w:r></w:p></w:body></w:document>"#).unwrap();
        zip.start_file("word/media/foto1.png", opts).unwrap();
        zip.write_all(&png).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn test_docx_reader_extracts_image() {
        let dir = std::env::temp_dir().join("oxt_docx_img");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("img.docx");
        write_docx_with_image(&path);

        let reader = DocxReader::open(&path).unwrap();
        let ir = reader.into_ir();
        let kinds: Vec<&str> = ir.sections[0]
            .elements
            .iter()
            .map(|e| match e {
                Element::Image { .. } => "image",
                Element::Paragraph { .. } => "paragraph",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["paragraph", "image", "paragraph"], "elementos: {kinds:?}");
        let img = &ir.sections[0].elements[1];
        if let Element::Image { filename, data, alt_text } = img {
            assert_eq!(filename, "foto1.png");
            assert_eq!(alt_text.as_deref(), Some("una foto"));
            let decoded = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
            assert_eq!(decoded, [0x89, 0x50, 0x4E, 0x47]);
        } else {
            panic!("esperaba Image");
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
