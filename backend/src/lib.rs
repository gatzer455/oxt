//! # oxt — motor minimalista para documentos de oficina
//!
//! Backend que transforma documentos DOCX/XLSX/PPTX/ODF en un IR unificado
//! (XiIR) que los LLMs pueden leer y manipular.
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
pub mod opc;
pub mod xlsx;
pub mod pptx;
pub mod legacy;

use std::path::Path;

use ir::{DocumentFormat, XiIR};

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

    #[error("OPC error: {0}")]
    Opc(#[from] opc::OpcError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Un documento de oficina abierto (cualquier formato soportado).
pub struct Document {
    format: DocumentFormat,
    ir: XiIR,
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

        let ir = match fmt {
            DocumentFormat::Docx => {
                let reader = docx::DocxReader::open(path)?;
                reader.into_ir()
            }
            DocumentFormat::Xlsx => {
                let reader = xlsx::XlsxReader::open(path)?;
                reader.into_ir()
            }
            DocumentFormat::Pptx => {
                let reader = pptx::PptxReader::open(path)?;
                reader.into_ir()
            }
            DocumentFormat::Doc | DocumentFormat::Xls | DocumentFormat::Ppt => {
                let reader = legacy::LegacyReader::open(path)?;
                reader.into_ir()
            }
            DocumentFormat::Odt | DocumentFormat::Ods | DocumentFormat::Odp => {
                return Err(Error::UnsupportedFormat(format!(
                    "{}: ODF no implementado aún", fmt
                )));
            }
        };

        Ok(Self {
            format: fmt,
            ir,
            path: path.to_string_lossy().to_string(),
        })
    }

    /// Obtener el IR del documento.
    pub fn to_ir(&self) -> &XiIR {
        &self.ir
    }

    /// Consumir el documento y devolver el IR.
    pub fn into_ir(self) -> XiIR {
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
