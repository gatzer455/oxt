//! Lector para formatos ODF (.odt, .ods, .odp).
//!
//! Open Document Format (ISO 26300) usa un ZIP con `content.xml`
//! que contiene el contenido principal con namespaces específicos
//! (`office`, `text`, `table`, `draw`, etc.).

use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::ir::{DocumentFormat, Element, Metadata, Run, Section, OxtIR};

#[derive(Debug, thiserror::Error)]
pub enum OdfError {
    #[error("Error de ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Error de XML: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("Error de IO: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parte no encontrada: {0}")]
    PartNotFound(String),

    #[error("Formato ODF no soportado: {0}")]
    UnsupportedFormat(String),

    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

pub type Result<T> = std::result::Result<T, OdfError>;

/// Lector de documentos ODF.
pub struct OdfReader {
    ir: OxtIR,
}

impl OdfReader {
    /// Abrir y parsear un archivo ODF.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let fmt = DocumentFormat::from_path(path)
            .ok_or_else(|| OdfError::UnsupportedFormat(
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_string()
            ))?;

        // Verificar que sea ODF
        if !matches!(fmt, DocumentFormat::Odt | DocumentFormat::Ods | DocumentFormat::Odp) {
            return Err(OdfError::UnsupportedFormat(format!("{fmt}")));
        }

        let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;

        // Verificar mimetype
        let mut mime = String::new();
        archive.by_name("mimetype")?.read_to_string(&mut mime)?;
        let mime = mime.trim();
        let detected = match mime {
            "application/vnd.oasis.opendocument.text" => DocumentFormat::Odt,
            "application/vnd.oasis.opendocument.spreadsheet" => DocumentFormat::Ods,
            "application/vnd.oasis.opendocument.presentation" => DocumentFormat::Odp,
            other => return Err(OdfError::UnsupportedFormat(other.to_string())),
        };
        if detected != fmt {
            return Err(OdfError::UnsupportedFormat(
                format!("extensión {fmt:?} no coincide con mimetype {mime:?}")
            ));
        }

        // Leer content.xml
        let content_xml = Self::read_string(&mut archive, "content.xml")?;
        let elements = Self::parse_content(&content_xml, &fmt)?;

        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section { title: None, elements }],
        };

        Ok(Self { ir })
    }

    /// Consumir y devolver el IR.
    pub fn into_ir(self) -> OxtIR {
        self.ir
    }

    fn read_string(archive: &mut zip::ZipArchive<std::fs::File>, path: &str) -> Result<String> {
        let mut entry = archive
            .by_name(path)
            .map_err(|_| OdfError::PartNotFound(path.to_string()))?;
        let mut data = String::new();
        entry.read_to_string(&mut data)?;
        Ok(data)
    }

    fn parse_content(xml: &str, fmt: &DocumentFormat) -> Result<Vec<Element>> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().expand_empty_elements = true;
        reader.config_mut().trim_text(false);

        let mut buf = Vec::new();
        let mut elements: Vec<Element> = Vec::new();

        // Estado del parsing
        let mut in_body = false;
        let mut in_text_body = false;   // <office:text> (.odt)
        let mut in_spreadsheet = false; // <office:spreadsheet> (.ods)
        let mut in_presentation = false; // <office:presentation> (.odp)
        let mut in_draw_page = false;   // <draw:page> (.odp)
        let mut in_text_box = false;    // <draw:text-box> (.odp)

        let mut in_paragraph = false;   // <text:p> o <text:h>
        let mut in_heading = false;
        let mut current_heading_level: u8 = 1;

        let mut in_list = false;
        let mut list_items: Vec<String> = Vec::new();
        let mut list_ordered = false;
        let mut in_list_item = false;

        let mut in_table = false;
        let mut in_table_row = false;
        let mut in_table_cell = false;
        let mut table_rows: Vec<Vec<String>> = Vec::new();
        let mut table_row: Vec<String> = Vec::new();
        let mut cell_text = String::new();

        let mut current_text = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name_data = e.name().as_ref().to_owned();
                    let tag = local_name(&name_data);
                    match tag {
                        "body" => in_body = true,
                        "text" if in_body => in_text_body = true,
                        "spreadsheet" if in_body => in_spreadsheet = true,
                        "presentation" if in_body => in_presentation = true,

                        "p" if in_text_body || in_spreadsheet || in_text_box => {
                            in_paragraph = true;
                            current_text = String::new();
                        }
                        "h" if in_text_body => {
                            in_paragraph = true;
                            in_heading = true;
                            current_text = String::new();
                            // Leer nivel del heading desde atributo outline-level
                            current_heading_level = 1;
                            for attr in e.attributes() {
                                if let Ok(a) = attr {
                                    let aname = std::str::from_utf8(a.key.as_ref()).unwrap_or("");
                                    if aname == "outline-level" || aname.ends_with(":outline-level") {
                                        if let Ok(v) = std::str::from_utf8(&a.value) {
                                            current_heading_level = v.parse().unwrap_or(1);
                                        }
                                    }
                                }
                            }
                        }

                        "list" if in_text_body => {
                            in_list = true;
                            list_items = Vec::new();
                            list_ordered = false;
                            // Detectar si es ordered por tipo de lista
                            for attr in e.attributes() {
                                if let Ok(a) = attr {
                                    let aname = std::str::from_utf8(a.key.as_ref()).unwrap_or("");
                                    if aname == "style-name" || aname.ends_with(":style-name") {
                                        let val = std::str::from_utf8(&a.value).unwrap_or("");
                                        // Las listas numeradas usualmente usan estilos como "L1" o "Numbering_20_Symbols"
                                        if val.contains("Number") || val.contains("number") {
                                            list_ordered = true;
                                        }
                                    }
                                }
                            }
                        }
                        "list-item" if in_list => in_list_item = true,

                        "table" if in_text_body || in_spreadsheet => {
                            in_table = true;
                            table_rows = Vec::new();
                        }
                        "table-row" if in_table => {
                            in_table_row = true;
                            table_row = Vec::new();
                        }
                        "table-cell" if in_table_row => {
                            in_table_cell = true;
                            cell_text = String::new();
                        }

                        "page" if in_presentation => {
                            in_draw_page = true;
                        }
                        "frame" if in_draw_page => {
                            // draw:frame contiene draw:text-box
                        }
                        "text-box" if in_draw_page => {
                            in_text_box = true;
                        }

                        "span" if in_paragraph => {
                            // text:span — similar a runs, acumulamos texto igual
                        }
                        "s" if in_paragraph => {
                            // text:s — espacio, podemos ignorar o agregar espacio
                            if let Some(attr) = e.try_get_attribute("c").ok().flatten() {
                                if let Ok(count) = std::str::from_utf8(&attr.value) {
                                    if let Ok(n) = count.parse::<usize>() {
                                        current_text.push_str(&" ".repeat(n));
                                    }
                                }
                            } else {
                                current_text.push(' ');
                            }
                        }
                        "tab" if in_paragraph => {
                            current_text.push('\t');
                        }
                        "line-break" if in_paragraph => {
                            current_text.push('\n');
                        }

                        _ => {}
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let name_data = e.name().as_ref().to_owned();
                    let tag = local_name(&name_data);
                    match tag {
                        "s" if in_paragraph => {
                            if let Some(attr) = e.try_get_attribute("c").ok().flatten() {
                                if let Ok(count) = std::str::from_utf8(&attr.value) {
                                    if let Ok(n) = count.parse::<usize>() {
                                        current_text.push_str(&" ".repeat(n));
                                    }
                                }
                            } else {
                                current_text.push(' ');
                            }
                        }
                        "tab" if in_paragraph => current_text.push('\t'),
                        "line-break" if in_paragraph => current_text.push('\n'),
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape()?;
                    if in_paragraph {
                        current_text.push_str(&text);
                    } else if in_table_cell {
                        cell_text.push_str(&text);
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name_data = e.name().as_ref().to_owned();
                    let tag = local_name(&name_data);
                    match tag {
                        "body" => in_body = false,
                        "text" => in_text_body = false,
                        "spreadsheet" => in_spreadsheet = false,
                        "presentation" => in_presentation = false,

                        "p" if in_paragraph => {
                            in_paragraph = false;
                            if in_table_cell {
                                // Texto dentro de celda de tabla
                                cell_text.push_str(&current_text);
                            } else if in_list_item {
                                list_items.push(current_text.clone());
                            } else if in_text_box {
                                elements.push(Element::Paragraph {
                                    runs: vec![Run::plain(&current_text)],
                                });
                            } else {
                                elements.push(Element::Paragraph {
                                    runs: vec![Run::plain(&current_text)],
                                });
                            }
                            current_text = String::new();
                        }
                        "h" if in_heading => {
                            in_paragraph = false;
                            in_heading = false;
                            elements.push(Element::Heading {
                                level: current_heading_level,
                                text: current_text.clone(),
                            });
                            current_text = String::new();
                        }

                        "list" if in_list => {
                            if !list_items.is_empty() {
                                elements.push(Element::List {
                                    ordered: list_ordered,
                                    items: list_items.clone(),
                                    
                                });
                            }
                            in_list = false;
                            list_items = Vec::new();
                        }
                        "list-item" if in_list_item => {
                            in_list_item = false;
                        }

                        "table" if in_table => {
                            if !table_rows.is_empty() {
                                elements.push(Element::Table { rows: table_rows.clone() });
                            }
                            in_table = false;
                        }
                        "table-row" if in_table_row => {
                            if !table_row.is_empty() {
                                table_rows.push(table_row.clone());
                            }
                            in_table_row = false;
                        }
                        "table-cell" if in_table_cell => {
                            table_row.push(cell_text.trim().to_string());
                            in_table_cell = false;
                            cell_text = String::new();
                        }

                        "page" if in_draw_page => {
                            in_draw_page = false;
                        }
                        "text-box" if in_text_box => {
                            in_text_box = false;
                        }
                        "frame" => {}

                        "span" => {
                            // text:span — fin del span, no hay acción extra
                        }

                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(OdfError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(elements)
    }
}

/// Obtener el nombre local de un tag (sin namespace).
fn local_name(name: &[u8]) -> &str {
    let name = std::str::from_utf8(name).unwrap_or("");
    if let Some(pos) = name.find(':') {
        &name[pos + 1..]
    } else {
        name
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Probar local_name con prefijos ODF típicos.
    #[test]
    fn test_local_name_odf() {
        assert_eq!(local_name(b"text:p"), "p");
        assert_eq!(local_name(b"text:h"), "h");
        assert_eq!(local_name(b"table:table"), "table");
        assert_eq!(local_name(b"office:text"), "text");
        assert_eq!(local_name(b"draw:page"), "page");
    }

    /// Probar parsing de content.xml de .odt (mínimo).
    #[test]
    fn test_parse_odt_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content 
  xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0">
  <office:body>
    <office:text>
      <text:h text:outline-level="1">Título</text:h>
      <text:p>Párrafo uno</text:p>
      <text:p>Párrafo dos</text:p>
    </office:text>
  </office:body>
</office:document-content>"#;

        let elements = OdfReader::parse_content(xml, &DocumentFormat::Odt).unwrap();
        assert_eq!(elements.len(), 3);
        match &elements[0] {
            Element::Heading { level, text } => {
                assert_eq!(*level, 1);
                assert_eq!(text, "Título");
            }
            other => panic!("Esperaba Heading, obtuve {other:?}"),
        }
        match &elements[1] {
            Element::Paragraph { runs } => {
                assert_eq!(runs[0].text, "Párrafo uno");
            }
            other => panic!("Esperaba Paragraph, obtuve {other:?}"),
        }
        match &elements[2] {
            Element::Paragraph { runs } => {
                assert_eq!(runs[0].text, "Párrafo dos");
            }
            other => panic!("Esperaba Paragraph, obtuve {other:?}"),
        }
    }

    /// Probar parsing de tabla en .odt.
    #[test]
    fn test_parse_odt_table() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0">
  <office:body>
    <office:text>
      <table:table>
        <table:table-row>
          <table:table-cell><text:p>Nombre</text:p></table:table-cell>
          <table:table-cell><text:p>Edad</text:p></table:table-cell>
        </table:table-row>
        <table:table-row>
          <table:table-cell><text:p>Ana</text:p></table:table-cell>
          <table:table-cell><text:p>30</text:p></table:table-cell>
        </table:table-row>
      </table:table>
    </office:text>
  </office:body>
</office:document-content>"#;

        let elements = OdfReader::parse_content(xml, &DocumentFormat::Odt).unwrap();
        assert_eq!(elements.len(), 1);
        match &elements[0] {
            Element::Table { rows } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0], vec!["Nombre", "Edad"]);
                assert_eq!(rows[1], vec!["Ana", "30"]);
            }
            other => panic!("Esperaba Table, obtuve {other:?}"),
        }
    }

    /// Probar parsing de .ods (hoja de cálculo).
    #[test]
    fn test_parse_ods_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0">
  <office:body>
    <office:spreadsheet>
      <table:table>
        <table:table-row>
          <table:table-cell><text:p>A1</text:p></table:table-cell>
          <table:table-cell><text:p>B1</text:p></table:table-cell>
        </table:table-row>
        <table:table-row>
          <table:table-cell><text:p>A2</text:p></table:table-cell>
          <table:table-cell><text:p>B2</text:p></table:table-cell>
        </table:table-row>
      </table:table>
    </office:spreadsheet>
  </office:body>
</office:document-content>"#;

        let elements = OdfReader::parse_content(xml, &DocumentFormat::Ods).unwrap();
        assert_eq!(elements.len(), 1);
        match &elements[0] {
            Element::Table { rows } => {
                assert_eq!(rows[0], vec!["A1", "B1"]);
                assert_eq!(rows[1], vec!["A2", "B2"]);
            }
            other => panic!("Esperaba Table, obtuve {other:?}"),
        }
    }

    /// Probar parsing de .odp (presentación).
    #[test]
    fn test_parse_odp_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
  xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0">
  <office:body>
    <office:presentation>
      <draw:page>
        <draw:frame>
          <draw:text-box>
            <text:p>Slide 1 content</text:p>
          </draw:text-box>
        </draw:frame>
      </draw:page>
      <draw:page>
        <draw:frame>
          <draw:text-box>
            <text:p>Slide 2 content</text:p>
          </draw:text-box>
        </draw:frame>
      </draw:page>
    </office:presentation>
  </office:body>
</office:document-content>"#;

        let elements = OdfReader::parse_content(xml, &DocumentFormat::Odp).unwrap();
        assert_eq!(elements.len(), 2);
        match &elements[0] {
            Element::Paragraph { runs } => {
                assert_eq!(runs[0].text, "Slide 1 content");
            }
            other => panic!("Esperaba Paragraph, obtuve {other:?}"),
        }
    }

    /// Probar lista en .odt.
    #[test]
    fn test_parse_odt_list() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
  <office:body>
    <office:text>
      <text:list>
        <text:list-item><text:p>Item 1</text:p></text:list-item>
        <text:list-item><text:p>Item 2</text:p></text:list-item>
        <text:list-item><text:p>Item 3</text:p></text:list-item>
      </text:list>
    </office:text>
  </office:body>
</office:document-content>"#;

        let elements = OdfReader::parse_content(xml, &DocumentFormat::Odt).unwrap();
        assert_eq!(elements.len(), 1);
        match &elements[0] {
            Element::List { items, ordered, .. } => {
                assert!(!ordered);
                assert_eq!(items.len(), 3);
                assert_eq!(items[0], "Item 1");
                assert_eq!(items[1], "Item 2");
                assert_eq!(items[2], "Item 3");
            }
            other => panic!("Esperaba List, obtuve {other:?}"),
        }
    }
}
