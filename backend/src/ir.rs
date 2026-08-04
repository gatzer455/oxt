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
#[derive(Default)]
pub struct Metadata {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub page_count: Option<u32>,
    pub word_count: Option<u32>,
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
        self.plain_text_with_spans().0
    }

    /// Texto plano + spans con ruta IR y offset, en una sola pasada.
    ///
    /// Es el generador canónico: `plain_text()` y `search()` comparten esta
    /// travesía para que los offsets de grep sean consistentes con el texto.
    fn plain_text_with_spans(&self) -> (String, Vec<TextSpan>) {
        let mut out = String::new();
        let mut spans = Vec::new();

        for (si, section) in self.sections.iter().enumerate() {
            if let Some(ref title) = section.title {
                let start = out.len();
                out.push_str(title);
                spans.push(TextSpan {
                    start,
                    end: out.len(),
                    path: format!("/s[{si}]/title"),
                    text: title.clone(),
                    element_type: "title".into(),
                });
                out.push('\n');
                out.push_str(&"-".repeat(title.len()));
                out.push('\n');
            }

            for (ei, element) in section.elements.iter().enumerate() {
                match element {
                    Element::Heading { level, text } => {
                        out.push_str(&"#".repeat(*level as usize));
                        out.push(' ');
                        let start = out.len();
                        out.push_str(text);
                        spans.push(TextSpan {
                            start,
                            end: out.len(),
                            path: format!("/s[{si}]/h[{ei}]"),
                            text: text.clone(),
                            element_type: "heading".into(),
                        });
                        out.push('\n');
                    }
                    Element::Paragraph { runs } => {
                        for (ri, run) in runs.iter().enumerate() {
                            let start = out.len();
                            out.push_str(&run.text);
                            spans.push(TextSpan {
                                start,
                                end: out.len(),
                                path: format!("/s[{si}]/p[{ei}]/r[{ri}]"),
                                text: run.text.clone(),
                                element_type: "run".into(),
                            });
                        }
                        out.push('\n');
                    }
                    Element::Table { rows } => {
                        for (ri, row) in rows.iter().enumerate() {
                            for (ci, cell) in row.iter().enumerate() {
                                let start = out.len();
                                out.push_str(cell);
                                spans.push(TextSpan {
                                    start,
                                    end: out.len(),
                                    path: format!("/s[{si}]/t[{ei}]/r[{ri}]/c[{ci}]"),
                                    text: cell.clone(),
                                    element_type: "cell".into(),
                                });
                                out.push('\t');
                            }
                            out.push('\n');
                        }
                    }
                    Element::List { ordered, items } => {
                        for (ki, item) in items.iter().enumerate() {
                            if *ordered {
                                out.push_str(&format!("{}. ", ki + 1));
                            } else {
                                out.push_str("- ");
                            }
                            let start = out.len();
                            out.push_str(item);
                            spans.push(TextSpan {
                                start,
                                end: out.len(),
                                path: format!("/s[{si}]/l[{ei}]/i[{ki}]"),
                                text: item.clone(),
                                element_type: "list_item".into(),
                            });
                            out.push('\n');
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

        (out, spans)
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

// ── Stats, search y diff (verbos stats/grep/diff) ────────────────────────────

/// Métricas de un documento (`oxt stats`).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Stats {
    pub sections: usize,
    /// Conteo de elementos por tipo (kind del IR).
    pub elements: std::collections::BTreeMap<String, usize>,
    pub paragraphs: usize,
    pub words: usize,
    pub characters: usize,
    pub tables: usize,
    pub table_rows: usize,
    pub cells: usize,
    pub list_items: usize,
    pub images: usize,
    /// Headings por nivel (1..n).
    pub headings: std::collections::BTreeMap<u8, usize>,
    pub hyperlinks: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub per_section: Vec<SectionStats>,
}

/// Métricas por sección (`--per-section`).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SectionStats {
    pub title: Option<String>,
    pub elements: usize,
    pub words: usize,
}

/// Un match de `oxt grep`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchMatch {
    /// Texto que matcheó.
    pub text: String,
    /// Offset en chars sobre el texto plano del documento.
    pub offset: usize,
    /// Ruta IR del elemento (`/s[i]/p[j]/r[k]`, `/s[i]/t[j]/r[k]/c[l]`, …).
    pub path: String,
    /// Ventana de contexto de ±60 chars.
    pub context: String,
}

/// Un cambio de `oxt diff`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffChange {
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String, // "added" | "removed" | "modified"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
}

/// Reporte de `oxt diff`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffReport {
    pub equal: bool,
    pub changes: Vec<DiffChange>,
}

impl OxtIR {
    /// Métricas del documento (verbo `stats`).
    ///
    /// `per_section: true` agrega el desglose por sección.
    pub fn stats(&self, per_section: bool) -> Stats {
        let full = self.plain_text();
        let mut st = Stats {
            sections: self.sections.len(),
            words: full.split_whitespace().count(),
            characters: full.chars().count(),
            ..Default::default()
        };

        for section in &self.sections {
            let mut sec_words = 0usize;
            for element in &section.elements {
                let kind = match element {
                    Element::Heading { .. } => "heading",
                    Element::Paragraph { .. } => "paragraph",
                    Element::Table { .. } => "table",
                    Element::List { .. } => "list",
                    Element::Image { .. } => "image",
                    Element::ThematicBreak => "thematic_break",
                };
                *st.elements.entry(kind.to_string()).or_insert(0) += 1;
                sec_words += element.words();

                match element {
                    Element::Heading { level, text } => {
                        *st.headings.entry(*level).or_insert(0) += 1;
                        sec_words += text.split_whitespace().count();
                    }
                    Element::Paragraph { runs } => {
                        st.paragraphs += 1;
                        st.hyperlinks += runs.iter().filter(|r| r.hyperlink.is_some()).count();
                    }
                    Element::Table { rows } => {
                        st.tables += 1;
                        st.table_rows += rows.len();
                        st.cells += rows.iter().map(|r| r.len()).sum::<usize>();
                    }
                    Element::List { items, .. } => {
                        st.list_items += items.len();
                    }
                    Element::Image { .. } => {
                        st.images += 1;
                    }
                    _ => {}
                }
            }
            if per_section {
                st.per_section.push(SectionStats {
                    title: section.title.clone(),
                    elements: section.elements.len(),
                    words: sec_words,
                });
            }
        }
        st
    }

    /// Buscar un patrón en el documento (verbo `grep`).
    ///
    /// `literal: true` busca el texto tal cual; `case_insensitive` agrega `(?i)`.
    /// Los offsets son índices de chars sobre `plain_text()`.
    pub fn search(
        &self,
        pattern: &str,
        literal: bool,
        case_insensitive: bool,
    ) -> Result<Vec<SearchMatch>, regex::Error> {
        let (full, spans) = self.plain_text_with_spans();
        let pat = if literal {
            regex::escape(pattern)
        } else {
            pattern.to_string()
        };
        let re = if case_insensitive {
            regex::RegexBuilder::new(&pat).case_insensitive(true).build()?
        } else {
            regex::Regex::new(&pat)?
        };

        let mut out = Vec::new();
        for span in &spans {
            let slice = &full[span.start..span.end];
            for m in re.find_iter(slice) {
                let start = span.start + m.start();
                let end = span.start + m.end();
                let ctx = context_window(&full, start, end);
                out.push(SearchMatch {
                    text: m.as_str().to_string(),
                    offset: full[..start].chars().count(),
                    path: span.path.clone(),
                    context: ctx,
                });
            }
        }
        Ok(out)
    }

    /// Comparar con otro documento (verbo `diff`).
    ///
    /// Alineación por índice con lookahead de 1: si el elemento i difiere pero
    /// el i+1 del otro lado coincide con el i, es una inserción/eliminación, no
    /// una modificación en cascada. Sin LCS.
    pub fn diff(&self, other: &OxtIR) -> DiffReport {
        let mut changes = Vec::new();
        diff_sections(&self.sections, &other.sections, "", &mut changes);
        DiffReport {
            equal: changes.is_empty(),
            changes,
        }
    }
}

impl Element {
    /// Conteo de palabras del elemento (texto de runs/celdas/items).
    fn words(&self) -> usize {
        match self {
            Element::Paragraph { runs } => {
                runs.iter().map(|r| r.text.split_whitespace().count()).sum()
            }
            Element::Table { rows } => rows
                .iter()
                .flatten()
                .map(|c| c.split_whitespace().count())
                .sum(),
            Element::List { items, .. } => {
                items.iter().map(|i| i.split_whitespace().count()).sum()
            }
            _ => 0,
        }
    }

    /// Representación textual corta para diff.
    fn repr(&self) -> String {
        match self {
            Element::Heading { text, .. } => text.clone(),
            Element::Paragraph { runs } => runs.iter().map(|r| r.text.as_str()).collect(),
            Element::Table { rows } => rows
                .iter()
                .map(|r| r.join(" | "))
                .collect::<Vec<_>>()
                .join(" / "),
            Element::List { ordered, items } => {
                let prefix = if *ordered { "1." } else { "-" };
                format!("{prefix} {}", items.join(" / "))
            }
            Element::Image { filename, .. } => format!("[imagen: {filename}]"),
            Element::ThematicBreak => "---".into(),
        }
    }
}

/// Ventana de contexto ±60 chars, recortada a límites de char.
fn context_window(full: &str, start: usize, end: usize) -> String {
    let mut s = start.saturating_sub(60);
    while s > 0 && !full.is_char_boundary(s) {
        s += 1;
    }
    let mut e = (end + 60).min(full.len());
    while e < full.len() && !full.is_char_boundary(e) {
        e += 1;
    }
    full[s..e].to_string()
}

fn diff_sections(a: &[Section], b: &[Section], base: &str, changes: &mut Vec<DiffChange>) {
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if i < a.len() && j + 1 < b.len() && a[i] == b[j + 1] {
            changes.push(DiffChange {
                path: format!("{base}/s[{j}]"),
                kind: "added".into(),
                old: None,
                new: Some(section_repr(&b[j])),
            });
            j += 1;
        } else if j < b.len() && i + 1 < a.len() && a[i + 1] == b[j] {
            changes.push(DiffChange {
                path: format!("{base}/s[{i}]"),
                kind: "removed".into(),
                old: Some(section_repr(&a[i])),
                new: None,
            });
            i += 1;
        } else if i < a.len() && j < b.len() {
            if a[i].title != b[j].title {
                changes.push(DiffChange {
                    path: format!("{base}/s[{i}]/title"),
                    kind: "modified".into(),
                    old: Some(a[i].title.clone().unwrap_or_else(|| "(sin título)".into())),
                    new: Some(b[j].title.clone().unwrap_or_else(|| "(sin título)".into())),
                });
            }
            diff_elements(&a[i].elements, &b[j].elements, &format!("{base}/s[{i}]"), changes);
            i += 1;
            j += 1;
        } else if i >= a.len() {
            changes.push(DiffChange {
                path: format!("{base}/s[{j}]"),
                kind: "added".into(),
                old: None,
                new: Some(section_repr(&b[j])),
            });
            j += 1;
        } else {
            changes.push(DiffChange {
                path: format!("{base}/s[{i}]"),
                kind: "removed".into(),
                old: Some(section_repr(&a[i])),
                new: None,
            });
            i += 1;
        }
    }
}

fn diff_elements(a: &[Element], b: &[Element], base: &str, changes: &mut Vec<DiffChange>) {
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if i < a.len() && j + 1 < b.len() && a[i] == b[j + 1] {
            changes.push(DiffChange {
                path: format!("{base}/e[{j}]"),
                kind: "added".into(),
                old: None,
                new: Some(b[j].repr()),
            });
            j += 1;
        } else if j < b.len() && i + 1 < a.len() && a[i + 1] == b[j] {
            changes.push(DiffChange {
                path: format!("{base}/e[{i}]"),
                kind: "removed".into(),
                old: Some(a[i].repr()),
                new: None,
            });
            i += 1;
        } else if i < a.len() && j < b.len() {
            let old = a[i].repr();
            let new = if element_text(&a[i]) == element_text(&b[j]) {
                format!("{} (formato)", old)
            } else {
                b[j].repr()
            };
            changes.push(DiffChange {
                path: format!("{base}/e[{i}]"),
                kind: "modified".into(),
                old: Some(old),
                new: Some(new),
            });
            i += 1;
            j += 1;
        } else if i >= a.len() {
            changes.push(DiffChange {
                path: format!("{base}/e[{j}]"),
                kind: "added".into(),
                old: None,
                new: Some(b[j].repr()),
            });
            j += 1;
        } else {
            changes.push(DiffChange {
                path: format!("{base}/e[{i}]"),
                kind: "removed".into(),
                old: Some(a[i].repr()),
                new: None,
            });
            i += 1;
        }
    }
}

fn section_repr(s: &Section) -> String {
    s.title.clone().unwrap_or_else(|| "(sin título)".into())
}

/// Texto plano de un elemento (para comparar texto sin formato).
fn element_text(e: &Element) -> String {
    match e {
        Element::Heading { text, .. } => text.clone(),
        Element::Paragraph { runs } => runs.iter().map(|r| r.text.as_str()).collect(),
        Element::Table { rows } => rows
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join("\t"),
        Element::List { items, .. } => items.join("\n"),
        _ => String::new(),
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
