//! # PPTX Reader
//!
//! Parsea un archivo .pptx → OxtIR.
//!
//! Un PPTX es un ZIP con:
//!   - ppt/presentation.xml    → lista de diapositivas
//!   - ppt/slides/slide1.xml   → cada diapositiva (árbol de shapes)
//!   - ppt/media/              → imágenes
//!   - ppt/slideMasters/       → masters (para layouts)
//!   - ppt/_rels/presentation.xml.rels → relaciones

use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use std::collections::HashMap;

use crate::ir::*;
use crate::opc::OpcPackage;

/// Error del parser PPTX.
#[derive(Debug, thiserror::Error)]
pub enum PptxError {
    #[error("OPC error: {0}")]
    Opc(#[from] crate::opc::OpcError),

    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("Part not found: {0}")]
    PartNotFound(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PptxError>;

/// Información de una diapositiva.
struct SlideInfo {
    id: u32,
    rel_id: String,
    filename: String,
}

/// PPTX parseado.
pub struct PptxReader {
    ir: OxtIR,
}

impl PptxReader {
    /// Abrir y parsear un .pptx.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let mut pkg = OpcPackage::open(path.as_ref())?;
        Self::parse(&mut pkg)
    }

    /// Parsear desde un paquete OPC ya abierto.
    pub fn parse(pkg: &mut OpcPackage<std::fs::File>) -> Result<Self> {
        // 1. Obtener lista de diapositivas desde presentation.xml
        let slides = Self::parse_slide_list(pkg)?;

        // 2. Resolver rutas desde las relaciones
        let rels = pkg.part_rels("ppt/presentation.xml")?;
        let slide_paths: HashMap<String, String> = rels.iter()
            .filter(|r| r.rel_type.contains("slide"))
            .map(|r| {
                let path = OpcPackage::<std::fs::File>::resolve_target("ppt/presentation.xml", &r.target);
                (r.id.clone(), path)
            })
            .collect();

        // 3. Parsear cada diapositiva
        let mut sections = Vec::new();
        for slide in &slides {
            let path = slide_paths.get(&slide.rel_id)
                .ok_or_else(|| PptxError::Other(format!("No path for slide {}", slide.id)))?;

            let slide_xml = pkg.read_string(path)?;
            let elements = Self::parse_slide(&slide_xml)?;

            sections.push(Section {
                title: Some(format!("Slide {}", slide.id)),
                elements,
            });
        }

        let ir = OxtIR {
            metadata: Metadata::default(),
            sections,
        };

        Ok(Self { ir })
    }

    /// Consumir y devolver el IR.
    pub fn into_ir(self) -> OxtIR {
        self.ir
    }

    // ── Slide list ─────────────────────────────────────────────────────────

    /// Parsea ppt/presentation.xml → lista de slides con sus IDs y relaciones.
    fn parse_slide_list(pkg: &mut OpcPackage<std::fs::File>) -> Result<Vec<SlideInfo>> {
        let xml = pkg.read_string("ppt/presentation.xml")?;
        let mut reader = XmlReader::from_str(&xml);
        reader.config_mut().expand_empty_elements = true;
        let mut buf = Vec::new();

        let mut slides = Vec::new();
        let mut in_sld_id = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    // Buscar <p:sldId> o <sldId>
                    if name == b"sldId" || name == b"p:sldId" {
                        let id = e.try_get_attribute("id").ok().flatten()
                            .and_then(|a| {
                                let s = String::from_utf8_lossy(&a.value);
                                s.parse::<u32>().ok()
                            })
                            .unwrap_or(0);
                        let rel_id = e.try_get_attribute("r:id").ok().flatten()
                            .or_else(|| e.try_get_attribute("id").ok().flatten())
                            .map(|a| String::from_utf8_lossy(&a.value).to_string())
                            .unwrap_or_default();

                        slides.push(SlideInfo {
                            id,
                            rel_id,
                            filename: format!("slide{}.xml", id),
                        });
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(PptxError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(slides)
    }

    // ── Slide parser ───────────────────────────────────────────────────────

    /// Parsea el XML de una diapositiva y extrae los elementos de texto.
    fn parse_slide(xml: &str) -> Result<Vec<Element>> {
        let mut reader = XmlReader::from_str(xml);
        reader.config_mut().expand_empty_elements = true;
        reader.config_mut().trim_text(false);
        let mut buf = Vec::new();

        let mut elements = Vec::new();
        let mut in_sp = false;       // <p:sp> = shape con texto
        let mut in_txBody = false;   // <p:txBody> = text body
        let mut in_p = false;       // <a:p> = paragraph
        let mut in_r = false;       // <a:r> = run
        let mut in_t = false;       // <a:t> = text
        let mut in_rPr = false;     // <a:rPr> = run properties
        let mut in_pic = false;     // <p:pic> = picture

        let mut current_paragraph_runs: Vec<Run> = Vec::new();
        let mut current_run: Option<Run> = None;
        let mut current_bold = false;
        let mut current_italic = false;
        let mut current_font_size: Option<f32> = None;
        let mut current_color: Option<String> = None;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    match name.as_slice() {
                        b"sp" | b"p:sp" => {
                            in_sp = true;
                        }
                        b"txBody" | b"p:txBody" if in_sp => {
                            in_txBody = true;
                        }
                        b"p" | b"a:p" if in_txBody => {
                            in_p = true;
                            current_paragraph_runs = Vec::new();
                        }
                        b"r" | b"a:r" if in_p => {
                            in_r = true;
                            current_run = Some(Run::plain(""));
                        }
                        b"rPr" | b"a:rPr" if in_r => {
                            in_rPr = true;
                            current_bold = false;
                            current_italic = false;
                            current_font_size = None;
                            current_color = None;
                        }
                        b"t" | b"a:t" if in_r => {
                            in_t = true;
                        }
                        b"pic" | b"p:pic" if in_sp => {
                            in_pic = true;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    match name.as_slice() {
                        // Atributos de run properties en elementos vacíos
                        b"solidFill" | b"a:solidFill" if in_rPr => {}
                        b"srgbClr" | b"a:srgbClr" if in_rPr => {
                            current_color = e.try_get_attribute("val").ok().flatten()
                                .map(|a| String::from_utf8_lossy(&a.value).to_string());
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if in_t {
                        if let Ok(text) = e.unescape() {
                            if let Some(ref mut run) = current_run {
                                run.text.push_str(text.trim());
                            }
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().as_ref().to_vec();
                    match name.as_slice() {
                        b"rPr" | b"a:rPr" => {
                            in_rPr = false;
                        }
                        b"r" | b"a:r" => {
                            if let Some(mut run) = current_run.take() {
                                run.bold = if current_bold { Some(true) } else { None };
                                run.italic = if current_italic { Some(true) } else { None };
                                run.font_size = current_font_size;
                                run.color = current_color.clone();
                                if !run.text.trim().is_empty() {
                                    current_paragraph_runs.push(run);
                                }
                            }
                            in_r = false;
                            in_t = false;
                        }
                        b"p" | b"a:p" => {
                            if !current_paragraph_runs.is_empty() {
                                elements.push(Element::Paragraph {
                                    runs: current_paragraph_runs.clone(),
                                });
                            } else {
                                // Párrafo vacío = salto de línea
                                elements.push(Element::Paragraph {
                                    runs: vec![Run::plain("")],
                                });
                            }
                            in_p = false;
                        }
                        b"txBody" | b"p:txBody" => {
                            in_txBody = false;
                        }
                        b"sp" | b"p:sp" => {
                            in_sp = false;
                        }
                        b"pic" | b"p:pic" => {
                            in_pic = false;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(PptxError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(elements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_slide_empty() {
        // Slide mínimo sin shapes
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
       xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr>
        <p:cNvPr id="1" name=""/>
        <p:cNvGrpSpPr/>
        <p:nvPr/>
      </p:nvGrpSpPr>
      <p:grpSpPr/>
    </p:spTree>
  </p:cSld>
</p:sld>"#;
        let elements = PptxReader::parse_slide(xml).unwrap();
        assert!(elements.is_empty(), "slide vacío no debería tener elementos");
    }

    #[test]
    fn test_parse_slide_with_text() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
       xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:nvSpPr>
          <p:cNvPr id="2" name="Title 1"/>
          <p:nvSpPr/>
          <p:nvPr/>
        </p:nvSpPr>
        <p:spPr/>
        <p:txBody>
          <a:p>
            <a:r>
              <a:rPr b="1" i="0" sz="2400"/>
              <a:t>Hello World</a:t>
            </a:r>
          </a:p>
        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#;
        let elements = PptxReader::parse_slide(xml).unwrap();
        assert!(!elements.is_empty(), "debería haber elementos");

        if let Some(Element::Paragraph { runs }) = elements.first() {
            assert!(!runs.is_empty(), "debería haber runs");
            assert_eq!(runs[0].text, "Hello World");
        } else {
            panic!("Se esperaba un Paragraph");
        }
    }
}
