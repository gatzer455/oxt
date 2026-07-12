//! # OxtIR — Intermediate Representation unificado
//!
//! Este es el contrato entre el documento físico y el LLM.
//! Todo formato (DOCX, XLSX, PPTX, ODT…) se reduce a esta representación.
//! Serializable a JSON para que el agente lo entienda y lo pueda manipular.

use serde::{Deserialize, Serialize};

// ── OxtIR ─────────────────────────────────────────────────────────────────────

/// Representación unificada de un documento completo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OxtIR {
    #[serde(default)]
    pub metadata: Metadata,
    pub sections: Vec<Section>,
}

/// Metadatos del documento.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Metadata {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub page_count: Option<u32>,
    pub word_count: Option<u32>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            title: None,
            subject: None,
            creator: None,
            page_count: None,
            word_count: None,
        }
    }
}

/// Una sección del documento.
///
/// En DOCX cada `w:sectPr` delimita una sección.
/// En XLSX cada hoja (worksheet) es una sección.
/// En PPTX cada diapositiva es una sección.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub title: Option<String>,
    pub elements: Vec<Element>,
}

/// Un elemento dentro de una sección.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Element {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        runs: Vec<Run>,
    },
    Table {
        rows: Vec<Vec<String>>,
    },
    List {
        ordered: bool,
        items: Vec<String>,
    },
    Image {
        filename: String,
        data: String, // base64
        alt_text: Option<String>,
    },
    ThematicBreak,
}

/// Un "run" de texto con formato.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Run {
    pub text: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyperlink: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>, // hex, ej: "FF0000"
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: None,
            italic: None,
            underline: None,
            strikethrough: None,
            font_size: None,
            hyperlink: None,
            color: None,
        }
    }
}

// ── TextOffsetMap (para ediciones precisas del agente) ────────────────────────

/// Mapa de offset → ruta en el documento.
///
/// El LLM recibe el texto plano + este mapa. Cuando quiere cambiar algo,
/// busca el texto, obtiene la ruta exacta (p.ej. `/body/p[3]/r[1]`),
/// y puede modificarlo con precisión.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextOffsetMap {
    /// Texto plano completo del documento.
    pub full_text: String,

    /// Spans individuales con su ruta.
    pub spans: Vec<TextSpan>,

    /// Metadatos del mapa.
    pub meta: OffsetMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
    pub path: String,
    pub text: String,
    pub element_type: String, // "run" | "cell" | "slide_text" | ...
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffsetMeta {
    pub format: String, // "docx" | "xlsx" | "pptx" | ...
    pub total_chars: usize,
    pub total_spans: usize,
}

// ── Formatos soportados ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat {
    Docx,
    Xlsx,
    Pptx,
    Doc,
    Xls,
    Ppt,
    Odt,
    Ods,
    Odp,
}

impl DocumentFormat {
    /// Detectar formato por extensión de archivo.
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "docx" => Some(Self::Docx),
            "xlsx" => Some(Self::Xlsx),
            "pptx" => Some(Self::Pptx),
            "doc" => Some(Self::Doc),
            "xls" => Some(Self::Xls),
            "ppt" => Some(Self::Ppt),
            "odt" => Some(Self::Odt),
            "ods" => Some(Self::Ods),
            "odp" => Some(Self::Odp),
            _ => None,
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Doc => "doc",
            Self::Xls => "xls",
            Self::Ppt => "ppt",
            Self::Odt => "odt",
            Self::Ods => "ods",
            Self::Odp => "odp",
        }
    }
}

impl std::fmt::Display for DocumentFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.extension())
    }
}

// ── Rendering helpers ─────────────────────────────────────────────────────────

impl OxtIR {
    /// Renderizar a texto plano (pérdida de formato).
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for section in &self.sections {
            if let Some(ref title) = section.title {
                out.push_str(title);
                out.push('\n');
                out.push_str(&"-".repeat(title.len()));
                out.push('\n');
            }
            for element in &section.elements {
                match element {
                    Element::Heading { level, text } => {
                        out.push_str(&format!("{} {}", "#".repeat(*level as usize), text));
                        out.push('\n');
                    }
                    Element::Paragraph { runs } => {
                        for run in runs {
                            out.push_str(&run.text);
                        }
                        out.push('\n');
                    }
                    Element::Table { rows } => {
                        for row in rows {
                            out.push_str(&row.join("\t"));
                            out.push('\n');
                        }
                    }
                    Element::List { ordered, items } => {
                        for (i, item) in items.iter().enumerate() {
                            if *ordered {
                                out.push_str(&format!("{}. {}\n", i + 1, item));
                            } else {
                                out.push_str(&format!("- {}\n", item));
                            }
                        }
                    }
                    Element::Image { filename, alt_text, .. } => {
                        let alt = alt_text.as_deref().unwrap_or(filename);
                        out.push_str(&format!("[image: {}]\n", alt));
                    }
                    Element::ThematicBreak => {
                        out.push_str("---\n");
                    }
                }
            }
        }
        out
    }

    /// Renderizar a Markdown.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        for section in &self.sections {
            if let Some(ref title) = section.title {
                out.push_str(&format!("## {}\n\n", title));
            }
            for element in &section.elements {
                match element {
                    Element::Heading { level, text } => {
                        out.push_str(&format!("{} {}\n\n", "#".repeat(*level as usize), text));
                    }
                    Element::Paragraph { runs } => {
                        for run in runs {
                            let mut t = run.text.clone();
                            if run.bold.unwrap_or(false) {
                                t = format!("**{}**", t);
                            }
                            if run.italic.unwrap_or(false) {
                                t = format!("*{}*", t);
                            }
                            if let Some(ref url) = run.hyperlink {
                                t = format!("[{}]({})", t, url);
                            }
                            out.push_str(&t);
                        }
                        out.push('\n');
                        out.push('\n');
                    }
                    Element::Table { rows } => {
                        if rows.is_empty() {
                            continue;
                        }
                        // Header row
                        out.push('|');
                        for cell in &rows[0] {
                            out.push_str(&format!(" {} |", cell));
                        }
                        out.push('\n');
                        // Separator
                        out.push('|');
                        for _ in &rows[0] {
                            out.push_str(" --- |");
                        }
                        out.push('\n');
                        // Data rows
                        for row in &rows[1..] {
                            out.push('|');
                            for cell in row {
                                out.push_str(&format!(" {} |", cell));
                            }
                            out.push('\n');
                        }
                        out.push('\n');
                    }
                    Element::List { ordered, items } => {
                        for (i, item) in items.iter().enumerate() {
                            if *ordered {
                                out.push_str(&format!("{}. {}\n", i + 1, item));
                            } else {
                                out.push_str(&format!("- {}\n", item));
                            }
                        }
                        out.push('\n');
                    }
                    Element::Image { filename, data, alt_text } => {
                        let alt = alt_text.as_deref().unwrap_or(filename);
                        if !data.is_empty() {
                            out.push_str(&format!("![{}](data:image/png;base64,{})\n\n", alt, data));
                        } else {
                            out.push_str(&format!("![{}]({})\n\n", alt, filename));
                        }
                    }
                    Element::ThematicBreak => {
                        out.push_str("---\n\n");
                    }
                }
            }
        }
        out
    }

    /// Generar TextOffsetMap para ediciones precisas del agente.
    pub fn to_offset_map(&self, format: &str) -> TextOffsetMap {
        let mut full_text = String::new();
        let mut spans = Vec::new();
        let mut offset: usize = 0;

        for section in &self.sections {
            if let Some(ref title) = section.title {
                spans.push(TextSpan {
                    start: offset,
                    end: offset + title.len(),
                    path: "/meta/title".into(),
                    text: title.clone(),
                    element_type: "title".into(),
                });
                full_text.push_str(title);
                full_text.push('\n');
                offset = full_text.len();
            }

            for (elem_idx, element) in section.elements.iter().enumerate() {
                match element {
                    Element::Paragraph { runs } => {
                        for run in runs {
                            let start = offset;
                            full_text.push_str(&run.text);
                            offset = full_text.len();
                            spans.push(TextSpan {
                                start,
                                end: offset,
                                path: format!("/s[{}]/p[{}]/r[{}]",
                                    section.title.as_deref().unwrap_or("?"),
                                    elem_idx,
                                    spans.len()),
                                text: run.text.clone(),
                                element_type: "run".into(),
                            });
                        }
                        full_text.push('\n');
                        offset = full_text.len();
                    }
                    Element::Heading { text, .. } => {
                        let start = offset;
                        full_text.push_str(text);
                        offset = full_text.len();
                        spans.push(TextSpan {
                            start,
                            end: offset,
                            path: format!("/s[{}]/h[{}]", section.title.as_deref().unwrap_or("?"), elem_idx),
                            text: text.clone(),
                            element_type: "heading".into(),
                        });
                        full_text.push('\n');
                        offset = full_text.len();
                    }
                    Element::Table { rows } => {
                        for (ri, row) in rows.iter().enumerate() {
                            for (ci, cell) in row.iter().enumerate() {
                                let start = offset;
                                full_text.push_str(cell);
                                offset = full_text.len();
                                spans.push(TextSpan {
                                    start,
                                    end: offset,
                                    path: format!("/s[{}]/t[{}]/r[{}]/c[{}]",
                                        section.title.as_deref().unwrap_or("?"),
                                        elem_idx, ri, ci),
                                    text: cell.clone(),
                                    element_type: "cell".into(),
                                });
                                full_text.push('\t');
                                offset = full_text.len();
                            }
                            full_text.push('\n');
                            offset = full_text.len();
                        }
                    }
                    Element::List { items, .. } => {
                        for (i, item) in items.iter().enumerate() {
                            let start = offset;
                            full_text.push_str(item);
                            offset = full_text.len();
                            spans.push(TextSpan {
                                start,
                                end: offset,
                                path: format!("/s[{}]/l[{}]/i[{}]",
                                    section.title.as_deref().unwrap_or("?"),
                                    elem_idx, i),
                                text: item.clone(),
                                element_type: "list_item".into(),
                            });
                            full_text.push('\n');
                            offset = full_text.len();
                        }
                    }
                    _ => {}
                }
            }
        }

        let total_chars = full_text.len();
        let total_spans = spans.len();
        let format = format.to_string();

        TextOffsetMap {
            full_text,
            spans,
            meta: OffsetMeta {
                format,
                total_chars,
                total_spans,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ir_to_plain_text() {
        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: None,
                elements: vec![
                    Element::Heading { level: 1, text: "Título".into() },
                    Element::Paragraph { runs: vec![Run::plain("Hola mundo")] },
                ],
            }],
        };
        let text = ir.plain_text();
        assert!(text.contains("Título"));
        assert!(text.contains("Hola mundo"));
    }

    #[test]
    fn test_ir_to_markdown() {
        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: None,
                elements: vec![
                    Element::Heading { level: 2, text: "Sección".into() },
                    Element::Paragraph { runs: vec![
                        Run {
                            text: "negrita".into(),
                            bold: Some(true),
                            ..Default::default()
                        },
                    ]},
                ],
            }],
        };
        let md = ir.to_markdown();
        assert!(md.contains("## Sección"));
        assert!(md.contains("**negrita**"));
    }

    #[test]
    fn test_offset_map() {
        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: None,
                elements: vec![
                    Element::Paragraph { runs: vec![Run::plain("Hola")] },
                ],
            }],
        };
        let map = ir.to_offset_map("docx");
        assert_eq!(map.full_text, "Hola\n");
        assert_eq!(map.spans.len(), 1);
        assert_eq!(map.spans[0].text, "Hola");
        assert_eq!(map.spans[0].start, 0);
        assert_eq!(map.spans[0].end, 4);
    }
}
