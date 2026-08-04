//! # Media — extracción de imágenes a disco (verbo `media`)
//!
//! El reader ya embebe las imágenes en base64 dentro de `Element::Image`.
//! Este módulo las decodifica y las escribe a un directorio, con dedupe de
//! nombres. Para Google, `google.rs` provee los bytes vía sourceUri/contentUri.

use std::path::Path;

use base64::Engine;

use crate::ir::{Element, OxtIR};

/// Una imagen lista para escribir (bytes ya decodificados).
#[derive(Debug, Clone)]
pub struct MediaItem {
    pub filename: String,
    pub data: Vec<u8>,
    pub ir_path: String,
}

/// Archivo escrito.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaFile {
    pub file: String,
    pub filename: String,
    pub ir_path: String,
    pub bytes: usize,
}

/// Imagen que no se pudo extraer.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaSkipped {
    pub ir_path: String,
    pub reason: String,
}

/// Resultado de `oxt media`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaResult {
    pub files: Vec<MediaFile>,
    pub skipped: Vec<MediaSkipped>,
}

/// Error del módulo media.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("base64 inválido en {0}: {1}")]
    BadBase64(String, String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, MediaError>;

/// Recorrer el IR y recolectar las imágenes con su ruta.
/// Las que no decodifican base64 van a `skipped`.
pub fn collect_from_ir(ir: &OxtIR) -> (Vec<MediaItem>, Vec<MediaSkipped>) {
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for (si, section) in ir.sections.iter().enumerate() {
        for (ei, element) in section.elements.iter().enumerate() {
            if let Element::Image { filename, data, .. } = element {
                let ir_path = format!("/s[{si}]/e[{ei}]");
                match base64::engine::general_purpose::STANDARD.decode(data.as_bytes()) {
                    Ok(bytes) => items.push(MediaItem {
                        filename: filename.clone(),
                        data: bytes,
                        ir_path,
                    }),
                    Err(e) => skipped.push(MediaSkipped {
                        ir_path,
                        reason: format!("base64 inválido: {e}"),
                    }),
                }
            }
        }
    }
    (items, skipped)
}

/// Escribir imágenes a disco. Nombres duplicados → sufijo numérico
/// (`image1.png`, `image1_2.png`). Items vacíos → `skipped`.
pub fn write_media(items: Vec<MediaItem>, out_dir: &Path) -> Result<MediaResult> {
    std::fs::create_dir_all(out_dir)?;
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut used: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for item in items {
        if item.data.is_empty() {
            skipped.push(MediaSkipped {
                ir_path: item.ir_path.clone(),
                reason: "sin datos".into(),
            });
            continue;
        }

        // Nombre con dedupe
        let stem = item.filename.clone();
        let count = used.entry(stem.clone()).or_insert(0);
        let final_name = if *count == 0 {
            stem.clone()
        } else {
            let (base, ext) = split_ext(&stem);
            format!("{base}_{}{}", *count + 1, ext)
        };
        *count += 1;

        let out_path = out_dir.join(&final_name);
        std::fs::write(&out_path, &item.data)?;
        files.push(MediaFile {
            file: out_path.to_string_lossy().to_string(),
            filename: final_name,
            ir_path: item.ir_path,
            bytes: item.data.len(),
        });
    }

    Ok(MediaResult { files, skipped })
}

fn split_ext(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((base, ext)) => (base.to_string(), format!(".{ext}")),
        None => (name.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Metadata, Run, Section};

    fn png_bytes() -> Vec<u8> {
        vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0x01]
    }

    #[test]
    fn test_write_media_dedupe() {
        let dir = std::env::temp_dir().join("oxt_media_test");
        let _ = std::fs::remove_dir_all(&dir);

        let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes());
        let ir = OxtIR {
            metadata: Metadata::default(),
            sections: vec![Section {
                title: None,
                elements: vec![
                    Element::Image {
                        filename: "img.png".into(),
                        data: b64.clone(),
                        alt_text: None,
                    },
                    Element::Image {
                        filename: "img.png".into(),
                        data: b64,
                        alt_text: None,
                    },
                    Element::Image {
                        filename: "rota.png".into(),
                        data: "!!!no-base64!!!".into(),
                        alt_text: None,
                    },
                    Element::Paragraph { runs: vec![Run::plain("hola")] },
                ],
            }],
        };

        let (items, skipped) = collect_from_ir(&ir);
        assert_eq!(items.len(), 2);
        assert_eq!(skipped.len(), 1);

        let res = write_media(items, &dir).unwrap();
        assert_eq!(res.files.len(), 2);
        assert_eq!(res.skipped.len(), 0);
        assert!(res.files[0].filename.ends_with(".png"));
        assert!(dir.join("img_2.png").exists(), "debe deduplicar");
        assert!(skipped[0].reason.contains("base64"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
