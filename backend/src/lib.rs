//! # oxt — motor minimalista para documentos de oficina
//!
//! Backend que transforma documentos DOCX/XLSX/PPTX/ODF en un IR unificado
//! (OxtIR) que los LLMs pueden leer y manipular.
//!
//! ## Uso básico
//!
//! ```rust,no_run
//! use oxt_backend::Document;
//!
//! let doc = Document::open("reporte.docx")?;
//! let ir = doc.to_ir();
//! println!("{}", ir.to_markdown());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod agent;
pub mod docx;
pub mod ir;
pub mod media;
pub mod opc;
pub mod origin;
pub mod xlsx;
pub mod create;
pub mod edit;
pub mod pptx;
pub mod legacy;
pub mod odf;
pub mod roundtrip;
pub mod google;

use std::path::Path;

use ir::{DocumentFormat, OxtIR};

/// Error unificado del backend.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Formato no soportado: {0}")]
    UnsupportedFormat(String),

    #[error("DOCX error: {0}")]
    Docx(#[from] docx::DocxError),

    #[error("XLSX error: {0}")]
    Xlsx(#[from] xlsx::XlsxError),

    #[error("PPTX error: {0}")]
    Pptx(#[from] pptx::PptxError),

    #[error("Legacy error: {0}")]
    Legacy(#[from] legacy::LegacyError),

    #[error("Edit error: {0}")]
    Edit(#[from] edit::EditError),

    #[error("Create error: {0}")]
    Create(#[from] create::CreateError),

    #[error("ODF error: {0}")]
    Odf(#[from] odf::OdfError),

    #[error("Google error: {0}")]
    Google(#[from] google::GoogleError),

    #[error("Roundtrip error: {0}")]
    Roundtrip(#[from] roundtrip::RoundtripError),

    #[error("OPC error: {0}")]
    Opc(#[from] opc::OpcError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Intentar abrir un documento con el lector específico de su formato.
fn try_open_primary(path: &std::path::Path, fmt: DocumentFormat) -> Result<OxtIR> {
    match fmt {
        DocumentFormat::Docx => {
            let reader = docx::DocxReader::open(path)?;
            Ok(reader.into_ir())
        }
        DocumentFormat::Xlsx => {
            let reader = xlsx::XlsxReader::open(path)?;
            Ok(reader.into_ir())
        }
        DocumentFormat::Pptx => {
            let reader = pptx::PptxReader::open(path)?;
            Ok(reader.into_ir())
        }
        DocumentFormat::Doc | DocumentFormat::Xls | DocumentFormat::Ppt => {
            let reader = legacy::LegacyReader::open(path)?;
            Ok(reader.into_ir())
        }
        DocumentFormat::Odt | DocumentFormat::Ods | DocumentFormat::Odp => {
            let reader = odf::OdfReader::open(path)?;
            Ok(reader.into_ir())
        }
    }
}

/// Abrir un documento desde bytes en memoria (stdin).
/// El formato se detecta por contenido (sniffing), no por extensión.
pub fn open_from_bytes(bytes: &[u8]) -> Result<Document> {
    let tmp = std::env::temp_dir().join(format!("oxt_stdin_{}.oxt", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    let fmt = sniff_format(&tmp)?;
    let ir = try_open_primary(&tmp, fmt)?;
    let _ = std::fs::remove_file(&tmp);
    Ok(Document {
        format: fmt,
        ir,
        path: "-".into(),
    })
}

/// Detectar el formato de un archivo por su contenido.
fn sniff_format(path: &std::path::Path) -> Result<DocumentFormat> {
    let head = std::fs::read(path).unwrap_or_default();
    if head.starts_with(b"PK") {
        // ZIP → OOXML/ODF: probar readers en orden
        for f in [
            DocumentFormat::Docx,
            DocumentFormat::Xlsx,
            DocumentFormat::Pptx,
            DocumentFormat::Odt,
            DocumentFormat::Ods,
            DocumentFormat::Odp,
        ] {
            if try_open_primary(path, f).is_ok() {
                return Ok(f);
            }
        }
    } else if legacy::is_cfb(path) {
        for f in [DocumentFormat::Doc, DocumentFormat::Xls, DocumentFormat::Ppt] {
            if try_open_primary(path, f).is_ok() {
                return Ok(f);
            }
        }
    }
    Err(Error::UnsupportedFormat("bytes de stdin".into()))
}

/// Un documento de oficina abierto (cualquier formato soportado).
pub struct Document {
    format: DocumentFormat,
    ir: OxtIR,
    path: String,
}

impl Document {
    /// Abrir un documento desde una ruta de archivo.
    /// El formato se detecta por extensión.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let fmt = DocumentFormat::from_path(path)
            .ok_or_else(|| Error::UnsupportedFormat(
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("(sin extensión)")
                    .to_string()
            ))?;

        // Intentar el lector primario; si falla y es legacy,
        // probar OOXML como fallback
        let ir = try_open_primary(path, fmt)
            .or_else(|primary_err| {
                // Para extensiones legacy, probar OOXML como fallback
                let fallback_fmt = match fmt {
                    DocumentFormat::Doc => Some(DocumentFormat::Docx),
                    DocumentFormat::Xls => Some(DocumentFormat::Xlsx),
                    DocumentFormat::Ppt => Some(DocumentFormat::Pptx),
                    _ => None,
                };
                if let Some(fb) = fallback_fmt {
                    try_open_primary(path, fb).map_err(|_| primary_err)
                } else {
                    Err(primary_err)
                }
            })?;

        Ok(Self {
            format: fmt,
            ir,
            path: path.to_string_lossy().to_string(),
        })
    }

    /// Obtener el IR del documento.
    pub fn to_ir(&self) -> &OxtIR {
        &self.ir
    }

    /// Consumir el documento y devolver el IR.
    pub fn into_ir(self) -> OxtIR {
        self.ir
    }

    /// Formato del documento.
    pub fn format(&self) -> DocumentFormat {
        self.format
    }

    /// Ruta del archivo.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Texto plano.
    pub fn plain_text(&self) -> String {
        self.ir.plain_text()
    }

    /// Markdown.
    pub fn to_markdown(&self) -> String {
        self.ir.to_markdown()
    }
}
