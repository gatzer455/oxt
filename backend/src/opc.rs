//! # OPC Layer (Open Packaging Conventions)
//!
//! Los formatos OOXML (DOCX, XLSX, PPTX) son archivos ZIP que siguen
//! las convenciones OPC: [Content_Types].xml + .rels para relaciones.

use std::collections::HashMap;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;

/// Error del OPC layer.
#[derive(Debug, thiserror::Error)]
pub enum OpcError {
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("Part not found: {0}")]
    PartNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, OpcError>;

/// Una relación dentro del paquete OPC.
#[derive(Debug, Clone)]
pub struct Relationship {
    pub id: String,
    pub rel_type: String,
    pub target: String,
    pub target_mode: Option<String>,
}

/// Paquete OPC: un ZIP con semántica OPC.
pub struct OpcPackage<R: Read + Seek> {
    archive: zip::ZipArchive<R>,
    pub package_rels: Vec<Relationship>,
    content_types: HashMap<String, String>,
}

impl OpcPackage<std::fs::File> {
    /// Abrir un archivo OPC (docx/xlsx/pptx).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = std::fs::File::open(path.as_ref())?;
        let mut pkg = Self {
            archive: zip::ZipArchive::new(file)?,
            package_rels: Vec::new(),
            content_types: HashMap::new(),
        };
        pkg.load_metadata()?;
        Ok(pkg)
    }
}

impl<R: Read + Seek> OpcPackage<R> {
    /// Crear desde un reader (útil para tests).
    pub fn from_reader(reader: R) -> Result<Self> {
        let mut pkg = Self {
            archive: zip::ZipArchive::new(reader)?,
            package_rels: Vec::new(),
            content_types: HashMap::new(),
        };
        pkg.load_metadata()?;
        Ok(pkg)
    }

    fn load_metadata(&mut self) -> Result<()> {
        self.content_types = self.parse_content_types()?;
        self.package_rels = self.parse_rels("_rels/.rels")?;
        Ok(())
    }

    // ── Content Types ──────────────────────────────────────────────────────

    fn parse_content_types(&mut self) -> Result<HashMap<String, String>> {
        let data = self.archive.by_name("[Content_Types].xml")
            .map_err(|_| OpcError::PartNotFound("[Content_Types].xml".into()))?;
        let reader = BufReader::new(data);
        let mut xml = XmlReader::from_reader(reader);
        xml.config_mut().expand_empty_elements = true;

        let mut types = HashMap::new();
        let mut buf = Vec::new();

        loop {
            match xml.read_event_into(&mut buf) {
                Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                    if e.name().as_ref() == b"Default" {
                        if let (Some(ext), Some(ct)) = (
                            e.try_get_attribute("Extension").ok().flatten(),
                            e.try_get_attribute("ContentType").ok().flatten(),
                        ) {
                            let ext = String::from_utf8_lossy(&ext.value).to_lowercase();
                            let ct = String::from_utf8_lossy(&ct.value).to_string();
                            types.insert(format!(".{}", ext), ct);
                        }
                    } else if e.name().as_ref() == b"Override" {
                        if let (Some(part), Some(ct)) = (
                            e.try_get_attribute("PartName").ok().flatten(),
                            e.try_get_attribute("ContentType").ok().flatten(),
                        ) {
                            let part = String::from_utf8_lossy(&part.value).to_lowercase();
                            let ct = String::from_utf8_lossy(&ct.value).to_string();
                            types.insert(part, ct);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(OpcError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(types)
    }

    // ── Relationships ───────────────────────────────────────────────────────

    fn parse_rels(&mut self, path: &str) -> Result<Vec<Relationship>> {
        let data = match self.archive.by_name(path) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(Vec::new()),
            Err(e) => return Err(OpcError::Zip(e)),
        };

        let reader = BufReader::new(data);
        let mut xml = XmlReader::from_reader(reader);
        let mut rels = Vec::new();
        let mut buf = Vec::new();

        loop {
            match xml.read_event_into(&mut buf) {
                Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                    if e.name().as_ref() == b"Relationship" {
                        let id = e.try_get_attribute("Id").ok().flatten()
                            .map(|a| String::from_utf8_lossy(&a.value).to_string());
                        let rel_type = e.try_get_attribute("Type").ok().flatten()
                            .map(|a| String::from_utf8_lossy(&a.value).to_string());
                        let target = e.try_get_attribute("Target").ok().flatten()
                            .map(|a| String::from_utf8_lossy(&a.value).to_string());
                        let target_mode = e.try_get_attribute("TargetMode").ok().flatten()
                            .map(|a| String::from_utf8_lossy(&a.value).to_string());

                        if let (Some(id), Some(rel_type), Some(target)) = (id, rel_type, target) {
                            rels.push(Relationship { id, rel_type, target, target_mode });
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(OpcError::Xml(e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(rels)
    }

    /// Obtener relaciones de una parte específica.
    pub fn part_rels(&mut self, part_path: &str) -> Result<Vec<Relationship>> {
        let path = std::path::Path::new(part_path);
        let dir = path.parent().unwrap_or_else(|| std::path::Path::new(""));
        let filename = path.file_name().unwrap().to_str().unwrap_or("");
        let rels_path = if dir.to_str().unwrap_or("").is_empty() {
            format!("_rels/{}.rels", filename)
        } else {
            format!("{}/_rels/{}.rels", dir.to_str().unwrap_or(""), filename)
        };
        self.parse_rels(&rels_path)
    }

    /// Leer una parte del ZIP como String.
    pub fn read_string(&mut self, path: &str) -> Result<String> {
        let normalized = path.replace('\\', "/");
        let mut entry = self.archive.by_name(&normalized)
            .map_err(|_| OpcError::PartNotFound(normalized.clone()))?;
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        Ok(String::from_utf8(data)?)
    }

    /// Leer una parte como bytes crudos.
    pub fn read_bytes(&mut self, path: &str) -> Result<Vec<u8>> {
        let normalized = path.replace('\\', "/");
        let mut entry = self.archive.by_name(&normalized)
            .map_err(|_| OpcError::PartNotFound(normalized.clone()))?;
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        Ok(data)
    }

    /// Verificar si una parte existe.
    pub fn has_part(&mut self, path: &str) -> bool {
        self.archive.by_name(path).is_ok()
    }

    /// Resolver el target de una relación contra el path base.
    pub fn resolve_target(base: &str, target: &str) -> String {
        let base_path = std::path::Path::new(base);
        let base_dir = base_path.parent().unwrap_or_else(|| std::path::Path::new(""));
        let resolved = base_dir.join(target);
        resolved.to_string_lossy().replace('\\', "/")
    }

    /// Listar todas las partes del ZIP.
    pub fn list_parts(&mut self) -> Vec<String> {
        self.archive.file_names().map(|n| n.to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_target() {
        assert_eq!(
            OpcPackage::<std::fs::File>::resolve_target("word/document.xml", "styles.xml"),
            "word/styles.xml"
        );
    }
}
