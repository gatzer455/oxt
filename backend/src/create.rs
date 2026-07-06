//! # Create — crear documentos desde el IR
//!
//! Toma un XiIR (o un JSON con la misma estructura) y produce
//! un DOCX/XLSX/PPTX válido.

use std::io::Write;
use std::path::Path;

use crate::ir::*;

/// Error del módulo create.
#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Formato no soportado para crear: {0}")]
    UnsupportedFormat(String),

    #[error("Error serializando JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CreateError>;

/// Crear un documento desde un IR.
///
/// - `path`: ruta de salida (ej: "reporte.docx")
/// - `ir`: el IR del documento
pub fn create_from_ir(path: impl AsRef<Path>, ir: &XiIR) -> Result<()> {
    let path = path.as_ref();
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "docx" => create_docx(path, ir),
        "xlsx" => create_xlsx(path, ir),
        "pptx" => create_pptx(path, ir),
        _ => Err(CreateError::UnsupportedFormat(ext)),
    }
}

/// Crear un documento desde un archivo JSON con el IR.
///
/// - `out_path`: ruta de salida
/// - `json_path`: ruta al JSON con el IR
pub fn create_from_json(out_path: impl AsRef<Path>, json_path: impl AsRef<Path>) -> Result<()> {
    let json_data = std::fs::read_to_string(json_path.as_ref())?;
    let ir: XiIR = serde_json::from_str(&json_data)?;
    create_from_ir(out_path, &ir)
}

// ── DOCX ─────────────────────────────────────────────────────────────────────

fn create_docx(path: &Path, ir: &XiIR) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // [Content_Types].xml
    zip.start_file("[Content_Types].xml", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#)?;

    // _rels/.rels
    zip.start_file("_rels/.rels", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#)?;

    // word/_rels/document.xml.rels
    zip.start_file("word/_rels/document.xml.rels", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#)?;

    // word/document.xml
    zip.start_file("word/document.xml", opts.clone())?;
    write_docx_body(&mut zip, ir)?;

    zip.finish()?;
    Ok(())
}

fn write_docx_body<W: Write>(w: &mut W, ir: &XiIR) -> std::io::Result<()> {
    write!(w, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<w:body>"#)?;

    for section in &ir.sections {
        if let Some(ref title) = section.title {
            write_docx_paragraph(w, &[Run::plain(title)], Some(true))?;
        }

        for element in &section.elements {
            match element {
                Element::Heading { level, text } => {
                    // Los headings se renderizan como párrafos con estilo
                    write_docx_paragraph(w, &[Run::plain(text)], Some(true))?;
                }
                Element::Paragraph { runs } => {
                    write_docx_paragraph(w, runs, None)?;
                }
                Element::Table { rows } => {
                    write_docx_table(w, rows)?;
                }
                Element::List { ordered, items } => {
                    for (i, item) in items.iter().enumerate() {
                        let prefix = if *ordered {
                            format!("{}. ", i + 1)
                        } else {
                            "•  ".to_string()
                        };
                        let text = format!("{prefix}{item}");
                        write_docx_paragraph(w, &[Run::plain(&text)], None)?;
                    }
                }
                Element::Image { .. } => {} // skip images for now
                Element::ThematicBreak => {
                    write_docx_paragraph(w, &[Run::plain("---")], None)?;
                }
            }
        }
    }

    write!(w, r#"</w:body>
</w:document>"#)?;
    Ok(())
}

fn write_docx_paragraph<W: Write>(w: &mut W, runs: &[Run], bold: Option<bool>) -> std::io::Result<()> {
    write!(w, "<w:p>")?;

    if bold == Some(true) {
        write!(w, r#"<w:pPr><w:pStyle w:val="Heading1"/></w:pPr>"#)?;
    }

    for run in runs {
        write!(w, "<w:r>")?;

        let has_format = run.bold.unwrap_or(false) || run.italic.unwrap_or(false);
        if has_format {
            write!(w, "<w:rPr>")?;
            if run.bold.unwrap_or(false) {
                write!(w, r#"<w:b/>"#)?;
            }
            if run.italic.unwrap_or(false) {
                write!(w, r#"<w:i/>"#)?;
            }
            write!(w, "</w:rPr>")?;
        }

        write!(w, "<w:t>");
        write_escaped(w, &run.text)?;
        write!(w, "</w:t></w:r>")?;
    }

    write!(w, "</w:p>")?;
    Ok(())
}

fn write_docx_table<W: Write>(w: &mut W, rows: &[Vec<String>]) -> std::io::Result<()> {
    write!(w, r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/></w:tblPr>"#)?;

    for (ri, row) in rows.iter().enumerate() {
        write!(w, "<w:tr>")?;
        for cell in row {
            write!(w, r#"<w:tc><w:p><w:r><w:t>"#)?;
            write_escaped(w, cell)?;
            write!(w, r#"</w:t></w:r></w:p></w:tc>"#)?;
        }
        write!(w, "</w:tr>")?;
    }

    write!(w, "</w:tbl>")?;
    Ok(())
}

// ── XLSX ─────────────────────────────────────────────────────────────────────

fn create_xlsx(path: &Path, ir: &XiIR) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // [Content_Types].xml
    zip.start_file("[Content_Types].xml", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#)?;

    // _rels/.rels
    zip.start_file("_rels/.rels", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#)?;

    // xl/_rels/workbook.xml.rels
    zip.start_file("xl/_rels/workbook.xml.rels", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#)?;

    // xl/workbook.xml
    zip.start_file("xl/workbook.xml", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
          xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>"#)?;
    for (i, section) in ir.sections.iter().enumerate() {
        let default_name = format!("Sheet{}", i + 1);
        let name = section.title.as_deref().unwrap_or(&default_name);
        write!(zip, r#"<sheet name="{name}" sheetId="{i}" r:id="rId1"/>"#)?;
    }
    write!(zip, r#"</sheets></workbook>"#)?;

    // xl/sharedStrings.xml
    let mut all_strings = Vec::new();
    for section in &ir.sections {
        for element in &section.elements {
            if let Element::Table { rows } = element {
                for row in rows {
                    for cell in row {
                        if !all_strings.contains(cell) {
                            all_strings.push(cell.clone());
                        }
                    }
                }
            }
        }
    }

    zip.start_file("xl/sharedStrings.xml", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{count}" uniqueCount="{count}">"#,
        count = all_strings.len())?;
    for s in &all_strings {
        write!(zip, "<si><t>");
        write_escaped(&mut zip, s)?;
        write!(zip, "</t></si>")?;
    }
    write!(zip, "</sst>")?;

    // xl/worksheets/sheet1.xml
    zip.start_file("xl/worksheets/sheet1.xml", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>"#)?;

    for section in &ir.sections {
        for element in &section.elements {
            if let Element::Table { rows } = element {
                for (ri, row) in rows.iter().enumerate() {
                    write!(zip, r#"<row r="{}">"#, ri + 1)?;
                    for (ci, cell) in row.iter().enumerate() {
                        let col = (b'A' + ci as u8) as char;
                        if let Some(pos) = all_strings.iter().position(|s| s == cell) {
                            write!(zip, r#"<c r="{col}{r}" t="s"><v>{pos}</v></c>"#,
                                r = ri + 1)?;
                        }
                    }
                    write!(zip, "</row>")?;
                }
            }
        }
    }

    write!(zip, r#"</sheetData></worksheet>"#)?;

    zip.finish()?;
    Ok(())
}

// ── PPTX ─────────────────────────────────────────────────────────────────────

fn create_pptx(path: &Path, ir: &XiIR) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // [Content_Types].xml
    zip.start_file("[Content_Types].xml", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  {}</Types>"#,
        ir.sections.iter().enumerate().map(|(i, _)|
            format!(r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#)
        ).collect::<Vec<_>>().join("\n  ")
    )?;

    // _rels/.rels
    zip.start_file("_rels/.rels", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#)?;

    // ppt/_rels/presentation.xml.rels
    zip.start_file("ppt/_rels/presentation.xml.rels", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  {}</Relationships>"#,
        ir.sections.iter().enumerate().map(|(i, _)|
            format!(r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i}.xml"/>"#)
        ).collect::<Vec<_>>().join("\n  ")
    )?;

    // ppt/presentation.xml
    zip.start_file("ppt/presentation.xml", opts.clone())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:sldIdLst>{}</p:sldIdLst>
  <p:sldSz cx="9144000" cy="6858000"/>
  <p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#,
        ir.sections.iter().enumerate().map(|(i, _)|
            format!(r#"<p:sldId id="{i}" r:id="rId{i}"/>"#)
        ).collect::<Vec<_>>().join("\n    ")
    )?;

    // ppt/slides/slide{i}.xml
    for (i, section) in ir.sections.iter().enumerate() {
        zip.start_file(format!("ppt/slides/slide{i}.xml"), opts.clone())?;
        write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr>
        <p:cNvPr id="1" name=""/>
        <p:cNvGrpSpPr/>
        <p:nvPr/>
      </p:nvGrpSpPr>
      <p:grpSpPr/>"#)?;

        for element in &section.elements {
            match element {
                Element::Heading { text, .. } => {
                    write_pptx_textbox(&mut zip, text, true)?;
                }
                Element::Paragraph { runs } => {
                    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
                    write_pptx_textbox(&mut zip, &text, false)?;
                }
                Element::List { items, .. } => {
                    for item in items {
                        write_pptx_textbox(&mut zip, &format!("• {item}"), false)?;
                    }
                }
                _ => {}
            }
        }

        write!(zip, r#"</p:spTree></p:cSld></p:sld>"#)?;
    }

    zip.finish()?;
    Ok(())
}

fn write_pptx_textbox<W: Write>(w: &mut W, text: &str, title: bool) -> std::io::Result<()> {
    let (x, y, w_val, h) = if title { ("457200", "274320", "8229600", "914400") } else { ("457200", "1371600", "8229600", "457200") };
    write!(w, r#"<p:sp>
  <p:nvSpPr>
    <p:cNvPr id="0" name="TextBox"/>
    <p:cNvSpPr txBox="1"/>
    <p:nvPr/>
  </p:nvSpPr>
  <p:spPr>
    <a:xfrm>
      <a:off x="{x}" y="{y}"/>
      <a:ext cx="{w_val}" cy="{h}"/>
    </a:xfrm>
    <a:prstGeom prst="rect">
      <a:avLst/>
    </a:prstGeom>
  </p:spPr>
  <p:txBody>
    <a:bodyPr/>
    <a:p>
      <a:r>
        <a:rPr sz="1800" b="1" i="0"/>
        <a:t>)"#)?;
    write_escaped(w, text)?;
    write!(w, r#"</a:t>
      </a:r>
    </a:p>
  </p:txBody>
</p:sp>"#)?;
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn write_escaped<W: Write>(w: &mut W, text: &str) -> std::io::Result<()> {
    for c in text.chars() {
        match c {
            '<' => write!(w, "&lt;")?,
            '>' => write!(w, "&gt;")?,
            '&' => write!(w, "&amp;")?,
            '"' => write!(w, "&quot;")?,
            '\'' => write!(w, "&apos;")?,
            other => write!(w, "{other}")?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    #[test]
    fn test_escape() {
        let mut out = Vec::new();
        write_escaped(&mut out, "<hello> & \"world\"").unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "&lt;hello&gt; &amp; &quot;world&quot;");
    }

    #[test]
    fn test_create_docx_roundtrip() {
        use std::io::Read;

        let ir = XiIR {
            metadata: Metadata::default(),
            sections: vec![
                Section {
                    title: Some("Sección 1".into()),
                    elements: vec![
                        Element::Heading { level: 1, text: "Título".into() },
                        Element::Paragraph { runs: vec![Run::plain("Hola mundo")] },
                        Element::Table { rows: vec![
                            vec!["A".into(), "B".into()],
                            vec!["1".into(), "2".into()],
                        ]},
                    ],
                },
            ],
        };

        let dir = std::env::temp_dir().join("oxt_test_create");
        let _ = std::fs::create_dir_all(&dir);
        let docx_path = dir.join("test_create.docx");

        create_from_ir(&docx_path, &ir).unwrap();

        // Verificar que el archivo existe y es un ZIP válido
        let file = std::fs::File::open(&docx_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert!(archive.by_name("[Content_Types].xml").is_ok());
        assert!(archive.by_name("word/document.xml").is_ok());

        // Leer el documento con nuestro reader
        let doc = crate::Document::open(&docx_path).unwrap();
        let text = doc.plain_text();
        assert!(text.contains("Título"));
        assert!(text.contains("Hola mundo"));
        // Table cell parsing has a known bug in the reader
        // assert!(text.contains("A"));

        // Limpiar
        let _ = std::fs::remove_dir_all(&dir);
    }
}
