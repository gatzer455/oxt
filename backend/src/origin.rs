//! # Origin — resolución de orígenes (path local / Google / stdin)
//!
//! Un verbo, cualquier origen: `read`, `edit`, `update`, … reciben un string
//! y este módulo decide qué es. Precedencia (contrato `docs/cli.md` §Orígenes):
//!
//! 1. `-` → stdin
//! 2. existe como path local → local
//! 3. URL de docs.google.com / drive.google.com con `/d/{id}` → Google con kind
//! 4. shape de ID de Google (`^[A-Za-z0-9_-]{25,}$`) → Google, kind por resolver
//! 5. nada → error `usage`

use std::path::PathBuf;

/// Qué tipo de documento de Google es un origen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoogleKind {
    Doc,
    Sheet,
    Slides,
}

impl GoogleKind {
    /// Kind a partir del mimeType de Drive API (`files.get`).
    pub fn from_mime(mime: &str) -> Option<Self> {
        match mime {
            "application/vnd.google-apps.document" => Some(Self::Doc),
            "application/vnd.google-apps.spreadsheet" => Some(Self::Sheet),
            "application/vnd.google-apps.presentation" => Some(Self::Slides),
            _ => None,
        }
    }
}

/// Un origen resuelto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `-`: leer de stdin.
    Stdin,
    /// Path local existente.
    Local(PathBuf),
    /// Documento de Google. `kind: None` = resolver en runtime (ID desnudo).
    Google { id: String, kind: Option<GoogleKind> },
}

/// Error de resolución de origen.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct OriginError(pub String);

/// Shape de un ID de Google (25+ chars alfanuméricos con `_`/`-`).
fn looks_like_google_id(s: &str) -> bool {
    s.len() >= 25 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Resolver un string de origen.
///
/// El kind de Google puede quedar `None` (URLs `file/d/{id}` o IDs desnudos);
/// se resuelve en runtime con `files.get` de Drive (ver `google.rs`).
pub fn resolve_origin(input: &str) -> Result<Origin, OriginError> {
    // 1. stdin
    if input == "-" {
        return Ok(Origin::Stdin);
    }

    // 2. path local existente
    let path = PathBuf::from(input);
    if path.exists() {
        return Ok(Origin::Local(path));
    }

    // 3. URL de Google
    for prefix in ["https://docs.google.com/", "http://docs.google.com/"] {
        if let Some(rest) = input.strip_prefix(prefix) {
            let parts: Vec<&str> = rest.split('/').collect();
            match parts.as_slice() {
                ["document", "d", id, ..] => {
                    return Ok(Origin::Google { id: id.to_string(), kind: Some(GoogleKind::Doc) });
                }
                ["spreadsheets", "d", id, ..] => {
                    return Ok(Origin::Google { id: id.to_string(), kind: Some(GoogleKind::Sheet) });
                }
                ["presentation", "d", id, ..] => {
                    return Ok(Origin::Google { id: id.to_string(), kind: Some(GoogleKind::Slides) });
                }
                ["file", "d", id, ..] => {
                    return Ok(Origin::Google { id: id.to_string(), kind: None });
                }
                _ => {
                    return Err(OriginError(format!(
                        "URL de Google no reconocida: {input} (se espera /document/d/…, /spreadsheets/d/…, /presentation/d/… o /file/d/…)"
                    )));
                }
            }
        }
    }
    if input.starts_with("https://drive.google.com/")
        || input.starts_with("http://drive.google.com/")
    {
        let rest = input.rsplit("/file/d/").next().unwrap_or("");
        let id = rest.split('/').next().unwrap_or("");
        if !id.is_empty() {
            return Ok(Origin::Google { id: id.to_string(), kind: None });
        }
    }

    // 4. ID desnudo
    if looks_like_google_id(input) {
        return Ok(Origin::Google { id: input.to_string(), kind: None });
    }

    // 5. nada
    Err(OriginError(format!(
        "origen no reconocido: {input} (se espera un path existente, una URL de Google o un ID de Google)"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdin() {
        assert_eq!(resolve_origin("-").unwrap(), Origin::Stdin);
    }

    #[test]
    fn test_local_path() {
        let p = std::env::temp_dir().join("oxt_origin_test.docx");
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(resolve_origin(p.to_str().unwrap()).unwrap(), Origin::Local(p.clone()));
        // Un path inexistente NO es local
        assert!(matches!(
            resolve_origin("/no/existe/asi.docx"),
            Err(_) | Ok(Origin::Google { .. })
        ));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_google_urls() {
        let cases = [
            (
                "https://docs.google.com/document/d/1A2B3C4D5E6F7G8H9I0J1K2L3M4N5O6P/edit",
                Some(GoogleKind::Doc),
            ),
            (
                "https://docs.google.com/spreadsheets/d/1A2B3C4D5E6F7G8H9I0J1K2L3M4N5O6P/edit#gid=0",
                Some(GoogleKind::Sheet),
            ),
            (
                "https://docs.google.com/presentation/d/1A2B3C4D5E6F7G8H9I0J1K2L3M4N5O6P/edit",
                Some(GoogleKind::Slides),
            ),
            (
                "https://docs.google.com/file/d/1A2B3C4D5E6F7G8H9I0J1K2L3M4N5O6P/view",
                None,
            ),
            (
                "https://drive.google.com/file/d/1A2B3C4D5E6F7G8H9I0J1K2L3M4N5O6P/view?usp=sharing",
                None,
            ),
        ];
        let id_expected = "1A2B3C4D5E6F7G8H9I0J1K2L3M4N5O6P";
        for (url, kind) in cases {
            match resolve_origin(url).unwrap() {
                Origin::Google { id, kind: got } => {
                    assert_eq!(id, id_expected, "id de {url}");
                    assert_eq!(got, kind, "kind de {url}");
                }
                other => panic!("esperaba Google, obtuve {other:?} para {url}"),
            }
        }
    }

    #[test]
    fn test_bare_id() {
        let id = "1A2B3C4D5E6F7G8H9I0J1K2L3M4N5O6P7Q8R9S0T";
        assert_eq!(
            resolve_origin(id).unwrap(),
            Origin::Google { id: id.to_string(), kind: None }
        );
        // IDs cortos no matchean
        assert!(resolve_origin("abc").is_err());
    }

    #[test]
    fn test_junk() {
        assert!(resolve_origin("!!!" ).is_err());
        assert!(resolve_origin("").is_err());
    }
}
