//! # Legacy format reader (.doc, .xls, .ppt)
//!
//! Abre archivos CFB (OLE2) y extrae texto básico.
//! No parsea estructura completa — suficiente para que el LLM
//! obtenga el contenido textual.

use std::io::Read;
use std::path::Path;

use crate::ir::*;

const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Error del parser legacy.
#[derive(Debug, thiserror::Error)]
pub enum LegacyError {
    #[error("No es un archivo CFB válido")]
    NotCfb,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Formato legacy no reconocido")]
    UnknownFormat,

    #[error("Stream '{0}' no encontrado")]
    StreamNotFound(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LegacyError>;

/// Legacy parseado.
pub struct LegacyReader {
    ir: OxtIR,
}

impl LegacyReader {
    /// Abrir y parsear un archivo legacy (.doc/.xls/.ppt).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Verificar magia CFB
        let mut file = std::fs::File::open(path)?;
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        if magic != CFB_MAGIC {
            return Err(LegacyError::NotCfb);
        }
        drop(file);

        // Abrir CFB
        let mut comp = cfb::open(path)?;

        // Detectar formato por streams
        let fmt = detect_format(&mut comp)?;

        // Extraer texto
        let (title, text) = match fmt {
            "doc" => ("Document", extract_doc_text(&mut comp)?),
            "xls" => ("Spreadsheet", extract_xls_text(&mut comp)?),
            "ppt" => ("Presentation", extract_ppt_text(&mut comp)?),
            _ => return Err(LegacyError::UnknownFormat),
        };

        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: Some(title.into()),
                elements: vec![Element::Paragraph { runs: vec![Run::plain(&text)] }],
            }],
        };

        Ok(Self { ir })
    }

    /// Consumir y devolver el IR.
    pub fn into_ir(self) -> OxtIR {
        self.ir
    }
}

// ── Detection ─────────────────────────────────────────────────────────────────

fn detect_format(comp: &mut cfb::CompoundFile<std::fs::File>) -> Result<&'static str> {
    // Listar streams en la raíz
    let entries = comp.read_storage("/")
        .map_err(|e| LegacyError::Other(format!("read_storage: {e}")))?;

    let names: Vec<String> = entries
        .map(|e| e.name().to_string())
        .collect();

    if names.iter().any(|n| n == "WordDocument" || n == "worddocument") {
        Ok("doc")
    } else if names.iter().any(|n| n == "Workbook" || n == "Book") {
        Ok("xls")
    } else if names.iter().any(|n| n == "PowerPoint Document") {
        Ok("ppt")
    } else {
        Err(LegacyError::UnknownFormat)
    }
}

// ── .doc ──────────────────────────────────────────────────────────────────────

fn extract_doc_text(comp: &mut cfb::CompoundFile<std::fs::File>) -> Result<String> {
    let stream_path = find_stream(comp, &["/WordDocument", "/worddocument"])?;
    let mut stream = comp.open_stream(&stream_path)
        .map_err(|_| LegacyError::StreamNotFound(stream_path.clone()))?;
    let mut data = Vec::new();
    stream.read_to_end(&mut data)?;

    if data.len() < 400 {
        return Ok(String::new());
    }

    let text_bytes = &data[400..];

    // Detectar UTF-16LE vs Latin-1
    let has_wide = text_bytes.chunks(2).any(|c| c.len() == 2 && c[0] == 0);
    let text = if has_wide {
        decode_utf16le(text_bytes)
    } else {
        text_bytes.iter()
            .take_while(|&&b| b != 0)
            .map(|&b| if b.is_ascii() || b >= 0xA0 { b as char } else { ' ' })
            .collect()
    };

    Ok(text.trim().replace('\x0C', "\n\n").replace('\x07', "\n"))
}

// ── .xls ──────────────────────────────────────────────────────────────────────

fn extract_xls_text(comp: &mut cfb::CompoundFile<std::fs::File>) -> Result<String> {
    let stream_path = find_stream(comp, &["/Workbook", "/Book", "/workbook", "/book"])?;
    let mut stream = comp.open_stream(&stream_path)
        .map_err(|_| LegacyError::StreamNotFound(stream_path.clone()))?;
    let mut data = Vec::new();
    stream.read_to_end(&mut data)?;

    let mut strings = Vec::new();
    let mut i = 0;

    while i + 4 <= data.len() {
        let rec_type = u16::from_le_bytes([data[i], data[i + 1]]);
        let rec_len = u16::from_le_bytes([data[i + 2], data[i + 3]]) as usize;
        let payload_end = i + 4 + rec_len;
        if payload_end > data.len() { break; }

        // Label (0x0204), LabelSST (0x00FD), RString (0x00D6)
        if rec_type == 0x00FD || rec_type == 0x0204 || rec_type == 0x00D6 {
            let text_start = i + 4 + 6;
            if text_start + 1 < payload_end {
                let span = &data[text_start..payload_end];
                let (text, _) = parse_short_text(span);
                if !text.is_empty() {
                    strings.push(text);
                }
            }
        }

        i = payload_end;
    }

    Ok(strings.join("\n"))
}

fn parse_short_text(data: &[u8]) -> (String, usize) {
    if data.is_empty() { return (String::new(), 0); }
    let len = data[0] as usize;
    if len == 0 || data.len() < 1 + len { return (String::new(), 1); }
    let text: String = data[1..1 + len].iter()
        .map(|&b| if b.is_ascii() || b >= 0xA0 { b as char } else { ' ' })
        .collect();
    (text, 1 + len)
}

// ── .ppt ──────────────────────────────────────────────────────────────────────

fn extract_ppt_text(comp: &mut cfb::CompoundFile<std::fs::File>) -> Result<String> {
    let stream_path = find_stream(comp, &["/PowerPoint Document"])?;
    let mut stream = comp.open_stream(&stream_path)
        .map_err(|_| LegacyError::StreamNotFound(stream_path.clone()))?;
    let mut data = Vec::new();
    stream.read_to_end(&mut data)?;

    let mut texts = Vec::new();
    let mut i = 0;

    while i + 8 <= data.len() {
        let rec_type = u16::from_le_bytes([data[i + 2], data[i + 3]]);
        let rec_len = u32::from_le_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]) as usize;
        let payload_end = i + 8 + rec_len;
        if payload_end > data.len() { break; }

        match rec_type {
            0x0FA0 => { // TextCharsAtom: UTF-16LE
                let t = decode_utf16le(&data[i + 8..payload_end]);
                if !t.trim().is_empty() { texts.push(t.trim().to_string()); }
            }
            0x0FA8 => { // TextBytesAtom: Latin-1
                let t: String = data[i + 8..payload_end].iter()
                    .take_while(|&&b| b != 0)
                    .map(|&b| if b.is_ascii() || b >= 0xA0 { b as char } else { ' ' })
                    .collect();
                if !t.trim().is_empty() { texts.push(t.trim().to_string()); }
            }
            _ => {}
        }

        i = if rec_len > 0 { payload_end } else { i + 8 };
    }

    Ok(texts.join("\n\n"))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn find_stream(comp: &mut cfb::CompoundFile<std::fs::File>, names: &[&str]) -> Result<String> {
    for &name in names {
        if comp.open_stream(name).is_ok() {
            return Ok(name.to_string());
        }
    }
    Err(LegacyError::StreamNotFound(names[0].to_string()))
}

fn decode_utf16le(bytes: &[u8]) -> String {
    let mut chars = Vec::new();
    for chunk in bytes.chunks(2) {
        if chunk.len() < 2 { break; }
        let code = u16::from_le_bytes([chunk[0], chunk[1]]);
        if code == 0 { break; }
        if let Some(c) = char::from_u32(code as u32) { chars.push(c); }
    }
    chars.into_iter().collect()
}

/// Verifica si un archivo es CFB (legacy Office).
pub fn is_cfb(path: impl AsRef<Path>) -> bool {
    if let Ok(mut file) = std::fs::File::open(path.as_ref()) {
        let mut magic = [0u8; 8];
        return file.read_exact(&mut magic).is_ok() && magic == CFB_MAGIC;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cfb_magic() {
        let zip_magic: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
        let cfb_magic: [u8; 4] = [0xD0, 0xCF, 0x11, 0xE0];
        assert_ne!(zip_magic, cfb_magic);
    }

    #[test]
    fn test_decode_utf16le() {
        assert_eq!(decode_utf16le(b"H\0e\0l\0l\0o\0"), "Hello");
        assert_eq!(decode_utf16le(b""), "");
    }

    #[test]
    fn test_parse_short_text() {
        let (text, _) = parse_short_text(&[5, b'H', b'e', b'l', b'l', b'o']);
        assert_eq!(text, "Hello");
    }
}
