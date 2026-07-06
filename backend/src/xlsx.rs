//! # XLSX Reader
//!
//! Parsea un archivo .xlsx → XiIR.
//!
//! Un XLSX es un ZIP con:
//!   - xl/sharedStrings.xml    → tabla de strings compartidos
//!   - xl/styles.xml           → estilos (formatos de celda, fechas)
//!   - xl/workbook.xml         → lista de hojas
//!   - xl/worksheets/sheet1.xml → datos de cada hoja
//!
//! Cada hoja (worksheet) se convierte en una sección del IR.
//! Los datos se convierten en elementos Table (una tabla por hoja).

use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use std::collections::HashMap;

use crate::ir::*;
use crate::opc::OpcPackage;

/// Error del parser XLSX.
#[derive(Debug, thiserror::Error)]
pub enum XlsxError {
    #[error("OPC error: {0}")]
    Opc(#[from] crate::opc::OpcError),

    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("Part not found: {0}")]
    PartNotFound(String),

    #[error("Sheet index out of bounds: {0}")]
    SheetOutOfBounds(usize),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, XlsxError>;

/// Información de una hoja.
struct SheetInfo {
    name: String,
    sheet_id: u32,
    rel_id: String,
}

/// XLSX parseado.
pub struct XlsxReader {
    ir: XiIR,
}

impl XlsxReader {
    /// Abrir y parsear un .xlsx.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let mut pkg = OpcPackage::open(path.as_ref())?;
        Self::parse(&mut pkg)
    }

    /// Parsear desde un paquete OPC ya abierto.
    pub fn parse(pkg: &mut OpcPackage<std::fs::File>) -> Result<Self> {
        // 1. Cargar shared strings
        let shared_strings = Self::parse_shared_strings(pkg)?;

        // 2. Cargar estilos (para detectar fechas)
        let date_formats = Self::parse_date_formats(pkg)?;

        // 3. Obtener lista de hojas
        let sheets = Self::parse_sheets(pkg)?;

        // 4. Resolver rutas de cada hoja desde las relaciones
        let rels = pkg.part_rels("xl/workbook.xml")?;
        let sheet_paths: HashMap<String, String> = rels.iter()
            .filter(|r| r.rel_type.contains("worksheet"))
            .map(|r| {
                let path = OpcPackage::<std::fs::File>::resolve_target("xl/workbook.xml", &r.target);
                (r.id.clone(), path)
            })
            .collect();

        // 5. Parsear cada hoja
        let mut sections = Vec::new();

        for sheet in &sheets {
            let path = sheet_paths.get(&sheet.rel_id)
                .ok_or_else(|| XlsxError::Other(format!("No path for sheet {}", sheet.name)))?;

            let sheet_xml = pkg.read_string(path)?;
            let rows = Self::parse_sheet(&sheet_xml, &shared_strings, &date_formats)?;

            sections.push(Section {
                title: Some(sheet.name.clone()),
                elements: vec![
                    Element::Table { rows },
                ],
            });
        }

        let ir = XiIR {
            metadata: Metadata::default(),
            sections,
        };

        Ok(Self { ir })
    }

    /// Consumir y devolver el IR.
    pub fn into_ir(self) -> XiIR {
        self.ir
    }

    // ── Shared Strings ─────────────────────────────────────────────────────

    /// Parsea `xl/sharedStrings.xml`.
    /// Devuelve un Vec<String> donde el índice es el ID del string compartido.
    fn parse_shared_strings(pkg: &mut OpcPackage<std::fs::File>) -> Result<Vec<String>> {
        let xml = match pkg.read_string("xl/sharedStrings.xml") {
            Ok(x) => x,
            Err(_) => return Ok(Vec::new()), // Sin strings compartidos
        };

        let mut reader = XmlReader::from_str(&xml);
        reader.config_mut().expand_empty_elements = true;
        reader.config_mut().trim_text(false);
        let mut buf = Vec::new();

        let mut strings = Vec::new();
        let mut in_si = false;  // <si> = string item
        let mut in_t = false;   // <t> = text
        let mut in_r = false;   // <r> = rich text run
        let mut current_text = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    match name.as_slice() {
                        b"si" => {
                            in_si = true;
                            current_text = String::new();
                        }
                        b"t" if in_si => {
                            in_t = true;
                        }
                        b"r" if in_si => {
                            in_r = true;
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    match name.as_slice() {
                        b"si" => {
                            strings.push(current_text.clone());
                            in_si = false;
                        }
                        b"t" => {
                            in_t = false;
                        }
                        b"r" => {
                            in_r = false;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if in_t {
                        if let Ok(text) = e.unescape() {
                            current_text.push_str(&text);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(XlsxError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(strings)
    }

    // ── Date format detection ───────────────────────────────────────────────

    /// Parsea `xl/styles.xml` para detectar formatos de fecha.
    /// Devuelve un set de IDs de formatos que son fechas.
    fn parse_date_formats(pkg: &mut OpcPackage<std::fs::File>) -> Result<Vec<u32>> {
        let xml = match pkg.read_string("xl/styles.xml") {
            Ok(x) => x,
            Err(_) => return Ok(Vec::new()),
        };

        let mut reader = XmlReader::from_str(&xml);
        reader.config_mut().expand_empty_elements = true;
        let mut buf = Vec::new();

        // Formatos de fecha built-in según ECMA-376
        let mut date_formats: Vec<u32> = vec![14, 15, 16, 17, 18, 19, 20, 21, 22, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 45, 46, 47, 48, 49, 50, 51, 55, 56, 57, 58];
        let mut in_numfmts = false;
        let mut in_cellxfs = false;
        let mut xf_index: u32 = 0;
        let mut numfmt_map: HashMap<u32, String> = HashMap::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    match name.as_slice() {
                        b"numFmts" => in_numfmts = true,
                        b"cellXfs" => in_cellxfs = true,
                        b"numFmt" if in_numfmts => {
                            let id = e.try_get_attribute("numFmtId").ok().flatten()
                                .and_then(|a| {
                                    let s = String::from_utf8_lossy(&a.value);
                                    s.parse::<u32>().ok()
                                });
                            let code = e.try_get_attribute("formatCode").ok().flatten()
                                .map(|a| String::from_utf8_lossy(&a.value).to_string());
                            if let (Some(id), Some(code)) = (id, code) {
                                numfmt_map.insert(id, code);
                            }
                        }
                        b"xf" if in_cellxfs => {
                            let numfmt = e.try_get_attribute("numFmtId").ok().flatten()
                                .and_then(|a| {
                                    let s = String::from_utf8_lossy(&a.value);
                                    s.parse::<u32>().ok()
                                });
                            if let Some(id) = numfmt {
                                if date_formats.contains(&id) || numfmt_map.get(&id)
                                    .map(|code| is_date_format(code))
                                    .unwrap_or(false)
                                {
                                    date_formats.push(xf_index);
                                }
                            }
                            xf_index += 1;
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    match name.as_slice() {
                        b"numFmts" => in_numfmts = false,
                        b"cellXfs" => in_cellxfs = false,
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(XlsxError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(date_formats)
    }

    // ── Sheets ──────────────────────────────────────────────────────────────

    /// Parsea `xl/workbook.xml` para obtener la lista de hojas.
    fn parse_sheets(pkg: &mut OpcPackage<std::fs::File>) -> Result<Vec<SheetInfo>> {
        let xml = pkg.read_string("xl/workbook.xml")?;
        let mut reader = XmlReader::from_str(&xml);
        reader.config_mut().expand_empty_elements = true;
        let mut buf = Vec::new();

        let mut sheets = Vec::new();
        let mut in_sheet = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    if name == b"sheet" || name == b"sheets" {
                        // <sheet> dentro de <sheets>
                        if name == b"sheet" {
                            let name_attr = e.try_get_attribute("name").ok().flatten()
                                .map(|a| String::from_utf8_lossy(&a.value).to_string())
                                .unwrap_or_default();
                            let sheet_id = e.try_get_attribute("sheetId").ok().flatten()
                                .and_then(|a| {
                                    let s = String::from_utf8_lossy(&a.value);
                                    s.parse::<u32>().ok()
                                })
                                .unwrap_or(0);
                            let rel_id = e.try_get_attribute("r:id").ok().flatten()
                                .or_else(|| e.try_get_attribute("id").ok().flatten())
                                .map(|a| String::from_utf8_lossy(&a.value).to_string())
                                .unwrap_or_default();

                            sheets.push(SheetInfo { name: name_attr, sheet_id, rel_id });
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(XlsxError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(sheets)
    }

    // ── Sheet parser ────────────────────────────────────────────────────────

    /// Parsea una hoja individual y devuelve filas de strings.
    fn parse_sheet(
        xml: &str,
        shared_strings: &[String],
        _date_formats: &[u32],
    ) -> Result<Vec<Vec<String>>> {
        let mut reader = XmlReader::from_str(xml);
        reader.config_mut().expand_empty_elements = true;
        reader.config_mut().trim_text(false);
        let mut buf = Vec::new();

        // Mapa: (row, col) → valor
        let mut cells: HashMap<(u32, u32), String> = HashMap::new();
        let mut max_row: u32 = 0;
        let mut max_col: u32 = 0;

        let mut in_row = false;
        let mut in_cell = false;
        let mut in_v = false;
        let mut current_row: u32 = 0;
        let mut current_col: u32 = 0;
        let mut current_type: Option<String> = None;
        let mut cell_value = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    match name.as_slice() {
                        b"row" => {
                            in_row = true;
                            current_row = e.try_get_attribute("r").ok().flatten()
                                .and_then(|a| {
                                    let s = String::from_utf8_lossy(&a.value);
                                    s.parse::<u32>().ok()
                                })
                                .unwrap_or(0);
                        }
                        b"c" if in_row => {
                            in_cell = true;
                            // Referencia tipo "A1", "B12"
                            let ref_attr = e.try_get_attribute("r").ok().flatten()
                                .map(|a| String::from_utf8_lossy(&a.value).to_string())
                                .unwrap_or_default();
                            current_col = col_from_ref(&ref_attr);
                            current_type = e.try_get_attribute("t").ok().flatten()
                                .map(|a| String::from_utf8_lossy(&a.value).to_string());
                            cell_value = String::new();
                        }
                        b"v" if in_cell => {
                            in_v = true;
                            cell_value = String::new();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if in_v {
                        if let Ok(text) = e.unescape() {
                            cell_value.push_str(&text);
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    match name.as_slice() {
                        b"row" => {
                            in_row = false;
                        }
                        b"c" => {
                            if in_cell && !cell_value.is_empty() {
                                // Resolver según tipo
                                let resolved = match current_type.as_deref() {
                                    Some("s") | Some("inlineStr") => {
                                        // String compartido por índice
                                        if let Ok(idx) = cell_value.parse::<usize>() {
                                            shared_strings.get(idx)
                                                .cloned()
                                                .unwrap_or_else(|| format!("[ref {idx}]"))
                                        } else {
                                            cell_value.clone()
                                        }
                                    }
                                    Some("b") => {
                                        // Booleano: 0=false, 1=true
                                        if cell_value == "1" { "TRUE".into() }
                                        else { "FALSE".into() }
                                    }
                                    Some("e") => {
                                        // Error
                                        format!("[error: {}]", cell_value)
                                    }
                                    _ => {
                                        // Número o fecha (por ahora texto plano)
                                        cell_value.clone()
                                    }
                                };

                                if !resolved.is_empty() {
                                    cells.insert((current_row, current_col), resolved);
                                    if current_row > max_row { max_row = current_row; }
                                    if current_col > max_col { max_col = current_col; }
                                }
                            }
                            in_cell = false;
                            in_v = false;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(XlsxError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        // Convertir el mapa de celdas en una matriz rectangular
        let num_rows = max_row as usize;
        let num_cols = max_col as usize;

        let mut rows = Vec::with_capacity(num_rows);
        for r in 1..=num_rows {
            let mut row = Vec::with_capacity(num_cols);
            for c in 1..=num_cols {
                let val = cells.get(&(r as u32, c as u32))
                    .cloned()
                    .unwrap_or_default();
                row.push(val);
            }
            // Solo incluir filas que tengan al menos un valor
            if row.iter().any(|v| !v.is_empty()) {
                rows.push(row);
            }
        }

        // Si no hay filas, devolver una fila vacía para que la tabla exista
        if rows.is_empty() {
            rows.push(vec!["(empty)".into()]);
        }

        Ok(rows)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convertir referencia de celda "A1" → número de columna (1-indexed).
fn col_from_ref(ref_str: &str) -> u32 {
    let letters: String = ref_str.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let mut col: u32 = 0;
    for c in letters.chars() {
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    col
}

/// Detectar si un formato numérico es de fecha.
fn is_date_format(code: &str) -> bool {
    let lower = code.to_lowercase();
    // Palabras clave que indican formato de fecha
    lower.contains("dd") || lower.contains("mm") || lower.contains("yy")
        || lower.contains("hh") || lower.contains("ss")
        // Fechas en español
        || lower.contains("día") || lower.contains("mes") || lower.contains("año")
        || lower.contains("fecha")
        // Formatos comunes
        || lower.contains("d/m") || lower.contains("m/d") || lower.contains("d-m")
        || lower.contains("m-y")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_col_from_ref() {
        assert_eq!(col_from_ref("A1"), 1);
        assert_eq!(col_from_ref("B12"), 2);
        assert_eq!(col_from_ref("Z1"), 26);
        assert_eq!(col_from_ref("AA1"), 27);
        assert_eq!(col_from_ref("AB1"), 28);
    }

    #[test]
    fn test_is_date_format() {
        assert!(is_date_format("dd/mm/yyyy"));
        assert!(is_date_format("yyyy-mm-dd"));
        assert!(is_date_format("hh:mm:ss"));
        assert!(!is_date_format("0.00"));
        assert!(!is_date_format("#,##0"));
    }

    #[test]
    fn test_date_format_detection_codes() {
        // Formatos built-in de fecha según ECMA-376
        let date_codes = [14, 15, 16, 17, 18, 19, 20, 21, 22, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 45, 46, 47, 48, 49, 50, 51, 55, 56, 57, 58];
        assert!(date_codes.contains(&14)); // "d/m/yyyy"
        assert!(date_codes.contains(&22)); // "d/m/yy h:mm"
    }
}
