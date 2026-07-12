//! # Google Workspace — conector REST para Docs, Sheets y Slides
//!
//! Permite leer y escribir documentos de Google Workspace a través de la
//! API REST oficial, mapeando todo al OxtIR.
//!
//! ## Autenticación
//!
//! ```bash
//! oxt google auth
//! ```
//!
//! Abre el navegador para autorizar la aplicación. El token se guarda en
//! `~/.config/oxt/google-tokens.json`.
//!
//! ## Uso
//!
//! ```bash
//! oxt google docs:read <document-id>
//! oxt google docs:create "Título"
//! oxt google docs:update <document-id> --from ir.json
//! ```
#![allow(dead_code, unused_variables)]
#[allow(unused_imports)]
use crate::ir::Element;

use std::path::PathBuf;

/// Error del módulo Google.
#[derive(Debug, thiserror::Error)]
pub enum GoogleError {
    #[error("HTTP error: {0}")]
    Http(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Autenticación requerida: ejecute 'oxt google auth' primero")]
    AuthRequired,

    #[error("Autenticación fallida: {0}")]
    AuthFailed(String),

    #[error("{0}")]
    Other(String),
}

#[cfg(feature = "google")]
pub type Result<T> = std::result::Result<T, GoogleError>;

#[cfg(not(feature = "google"))]
pub type Result<T> = std::result::Result<T, GoogleError>;

/// Token de acceso y refresh para Google APIs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoogleTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>, // UNIX timestamp
    pub client_id: String,
    pub client_secret: String,
}

/// Estado de autenticación.
pub enum AuthStatus {
    Authenticated,
    NotAuthenticated,
    Error(String),
}

// ── Auth ──────────────────────────────────────────────────────────────────────

/// Ruta al archivo de configuración de oxt.
fn config_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    base.join("oxt")
}

/// Ruta al archivo de tokens.
fn tokens_path() -> PathBuf {
    config_dir().join("google-tokens.json")
}

/// Cargar tokens guardados.
pub fn load_tokens() -> Result<GoogleTokens> {
    let path = tokens_path();
    if !path.exists() {
        return Err(GoogleError::AuthRequired);
    }
    let data = std::fs::read_to_string(&path)?;
    let tokens: GoogleTokens = serde_json::from_str(&data)?;

    // Verificar si el token expiró
    if let Some(exp) = tokens.expires_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        if now >= exp - 60 {
            // Token expirado o por expirar — refrescar
            return refresh_tokens(&tokens);
        }
    }

    Ok(tokens)
}

/// Refrescar token de acceso usando refresh_token.
fn refresh_tokens(old: &GoogleTokens) -> Result<GoogleTokens> {
    let refresh = old.refresh_token.as_deref()
        .ok_or_else(|| GoogleError::AuthFailed("No hay refresh_token".into()))?;

    #[cfg(feature = "google")]
    {
        let resp: serde_json::Value = ureq::post("https://oauth2.googleapis.com/token")
            .send_form([
                ("client_id", &old.client_id as &str),
                ("client_secret", &old.client_secret as &str),
                ("refresh_token", refresh as &str),
                ("grant_type", "refresh_token"),
            ])
            .map_err(|e| GoogleError::Http(e.to_string()))?
            .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

        let new_token = resp.get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GoogleError::AuthFailed("No access_token en respuesta".into()))?;

        let expires_in = resp.get("expires_in")
            .and_then(|v| v.as_i64())
            .unwrap_or(3600);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let tokens = GoogleTokens {
            access_token: new_token.to_string(),
            refresh_token: old.refresh_token.clone(),
            expires_at: Some(now + expires_in),
            client_id: old.client_id.clone(),
            client_secret: old.client_secret.clone(),
        };

        save_tokens(&tokens)?;
        return Ok(tokens);
    }

    #[cfg(not(feature = "google"))]
    Err(GoogleError::Other("Google feature no habilitada".into()))
}

/// Guardar tokens a disco.
fn save_tokens(tokens: &GoogleTokens) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let data = serde_json::to_string_pretty(tokens)?;
    std::fs::write(tokens_path(), data)?;
    Ok(())
}

/// Credenciales OAuth embebidas (Desktop app de GCP).
/// Estándar en CLIs de escritorio — Google no trata client_secret como secreto en desktop apps.
const DEFAULT_CLIENT_ID: &str = "327915843284-o2715l81t40re8568dineghb1t7kbqug.apps.googleusercontent.com";
const DEFAULT_CLIENT_SECRET: &str = "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";

/// Iniciar flujo OAuth2: abre navegador, recibe redirect en localhost.
///
/// Requiere credenciales de GCP: ir a https://console.cloud.google.com/apis/credentials
/// Crear una aplicación de escritorio con redirect URI: http://localhost:8080
pub fn authenticate(client_id: &str, client_secret: &str) -> Result<AuthStatus> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let redirect_port = 8080;
    let redirect_uri = format!("http://localhost:{redirect_port}");

    // PKCE: generar code_verifier + code_challenge
    use rand::Rng;
    use sha2::{Digest, Sha256};
    use base64::Engine as _;
    let verifier: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();
    let challenge = {
        let hash = Sha256::digest(verifier.as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
    };

    // Construir URL de autorización (scopes: docs, sheets, slides, drive.readonly)
    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?\
         client_id={client_id}&\
         redirect_uri={redirect_uri}&\
         response_type=code&\
         scope=https://www.googleapis.com/auth/documents%20\
                 https://www.googleapis.com/auth/spreadsheets%20\
                 https://www.googleapis.com/auth/presentations%20\
                 https://www.googleapis.com/auth/drive.readonly&\
         access_type=offline&\
         prompt=consent&\
         code_challenge_method=S256&\
         code_challenge={challenge}"
    );

    // Abrir navegador
    println!("Abriendo navegador para autorizar...");
    println!("Si no se abre, visita: {auth_url}");

    // Intentar abrir el navegador
    let _ = open_browser(&auth_url);

    // Escuchar el redirect en localhost
    let listener = TcpListener::bind(format!("127.0.0.1:{redirect_port}"))
        .map_err(|e| GoogleError::AuthFailed(format!("No se pudo abrir puerto {redirect_port}: {e}")))?;

    println!("Esperando autorización en http://localhost:{redirect_port}...");

    let code = loop {
        match listener.accept() {
        Ok((mut stream, _)) => {
            let mut buffer = [0; 8192];
            let n = match stream.read(&mut buffer) {
                Ok(n) => n,
                Err(_) => continue,
            };
            let request = String::from_utf8_lossy(&buffer[..n]);
            let request_line = request.lines().next().unwrap_or("");

            // Extraer el código de la query string. El formato puede ser:
            //   GET /?code=XXXX HTTP/1.1
            //   GET /?iss=...&code=XXXX&scope=... HTTP/1.1
            let extracted = request_line
                .split_whitespace()
                .nth(1)
                .and_then(|path_and_query| {
                    let query_start = path_and_query.find('?')?;
                    let query = &path_and_query[query_start + 1..];
                    for pair in query.split('&') {
                        if let Some(val) = pair.strip_prefix("code=") {
                            return Some(val.to_string());
                        }
                    }
                    None
                });

            if let Some(code_val) = extracted {
                let html_body = include_str!("google-pages/auth-success.html");
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n{html_body}");
                let _ = stream.write_all(response.as_bytes());
                break Some(code_val);
            } else {
                // Reenviar al navegador a Google para que intente de nuevo
                let response = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {auth_url}\r\nContent-Length: 0\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes());
            }
        }
        Err(e) => return Err(GoogleError::AuthFailed(format!("Error en redirect: {e}"))),
        }
    };

    let code = code.ok_or_else(|| GoogleError::AuthFailed("No se recibió código".into()))?;

    // Intercambiar código por tokens
    exchange_code_for_tokens(&code, client_id, client_secret, &redirect_uri, &verifier)?;

    Ok(AuthStatus::Authenticated)
}

/// Autenticar usando credenciales embebidas (DEFAULT_CLIENT_ID / DEFAULT_CLIENT_SECRET).
pub fn authenticate_defaults() -> Result<AuthStatus> {
    authenticate(DEFAULT_CLIENT_ID, DEFAULT_CLIENT_SECRET)
}

/// Intercambiar código de autorización por access_token + refresh_token.
fn exchange_code_for_tokens(
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<GoogleTokens> {
    #[cfg(feature = "google")]
    {
        let resp: serde_json::Value = ureq::post("https://oauth2.googleapis.com/token")
            .send_form([
                ("code", code),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("redirect_uri", redirect_uri),
                ("grant_type", "authorization_code"),
                ("code_verifier", code_verifier),
            ])
            .map_err(|e| GoogleError::Http(e.to_string()))?
            .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

        let access_token = resp.get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GoogleError::AuthFailed("No access_token".into()))?;

        let refresh_token = resp.get("refresh_token")
            .and_then(|v| v.as_str());

        let expires_in = resp.get("expires_in")
            .and_then(|v| v.as_i64())
            .unwrap_or(3600);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let tokens = GoogleTokens {
            access_token: access_token.to_string(),
            refresh_token: refresh_token.map(|s| s.to_string()),
            expires_at: Some(now + expires_in),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
        };

        save_tokens(&tokens)?;
        println!("✅ Autenticación exitosa");
        Ok(tokens)
    }

    #[cfg(not(feature = "google"))]
    Err(GoogleError::Other("Google feature no habilitada".into()))
}

#[cfg(target_os = "macos")]
fn open_browser(url: &str) -> Result<()> {
    std::process::Command::new("open").arg(url).spawn().ok();
    Ok(())
}

#[cfg(target_os = "linux")]
fn open_browser(url: &str) -> Result<()> {
    std::process::Command::new("xdg-open").arg(url).spawn().ok();
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_browser(url: &str) -> Result<()> {
    std::process::Command::new("cmd").args(["/c", "start", url]).spawn().ok();
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn open_browser(_url: &str) -> Result<()> {
    // No-op en plataformas no soportadas
    Ok(())
}

// ── Google Docs Reader ────────────────────────────────────────────────────────

/// Leer un Google Doc y devolverlo como OxtIR.
#[cfg(feature = "google")]
pub fn read_doc(document_id: &str) -> Result<crate::ir::OxtIR> {
    let tokens = load_tokens()?;
    let url = format!(
        "https://docs.googleapis.com/v1/documents/{document_id}"
    );

    let resp: serde_json::Value = ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .call()
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    doc_to_ir(&resp)
}

/// Convertir respuesta JSON de Google Docs a OxtIR.
#[cfg(feature = "google")]
fn doc_to_ir(doc: &serde_json::Value) -> Result<crate::ir::OxtIR> {
    use crate::ir::{Element, Metadata, Section};

    let title = doc.get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut elements: Vec<Element> = Vec::new();

    // Recorrer body.content[]
    if let Some(content) = doc.pointer("/body/content") {
        if let Some(items) = content.as_array() {
            for item in items {
                if let Some(elem) = parse_structural_element(item) {
                    elements.push(elem);
                }
            }
        }
    }

    let ir = crate::ir::OxtIR {
        metadata: Metadata {
            title,
            subject: None,
            creator: None,
            page_count: None,
            word_count: None,
        },
        sections: vec![Section {
            title: None,
            elements,
        }],
    };

    Ok(ir)
}

/// Parsear un structural element de Google Docs a Element.
#[cfg(feature = "google")]
fn parse_structural_element(item: &serde_json::Value) -> Option<Element> {
    if let Some(para) = item.get("paragraph") {
        return parse_paragraph(para);
    }
    if let Some(table) = item.get("table") {
        return parse_table(table);
    }
    if let Some(_section_break) = item.get("sectionBreak") {
        // Ignorar saltos de sección
    }
    None
}

/// Parsear un párrafo de Google Docs.
#[cfg(feature = "google")]
fn parse_paragraph(para: &serde_json::Value) -> Option<Element> {
    use crate::ir::Run;

    let mut runs: Vec<Run> = Vec::new();

    if let Some(elements) = para.get("elements") {
        if let Some(items) = elements.as_array() {
            for item in items {
                if let Some(text_run) = item.get("textRun") {
                    let content = text_run.get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Saltar nuevos items de lista (separadores)
                    if content == "\n" && runs.is_empty() {
                        continue;
                    }

                    let mut run = Run::plain(&content);

                    // Parsear formato del texto
                    if let Some(style) = text_run.get("textStyle") {
                        run.bold = style.get("bold").and_then(|v| v.as_bool());
                        run.italic = style.get("italic").and_then(|v| v.as_bool());
                        run.underline = style.get("underline").and_then(|v| v.as_bool());
                        run.strikethrough = style.get("strikethrough").and_then(|v| v.as_bool());

                        // Link
                        if let Some(link) = style.get("link") {
                            run.hyperlink = link.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                        }

                        // Color
                        if let Some(fg) = style.get("foregroundColor") {
                            if let Some(color) = fg.get("color") {
                                run.color = rgb_color_to_hex(color);
                            }
                        }

                        // Font size en half-points (Google Docs usa points * 2.67...)
                        // Realmente Google usa "weightedFontFamily.size" en magnitud (puntos * 2.67 aprox)
                        // No es straightforward, lo omitimos por ahora
                    }

                    runs.push(run);
                }
            }
        }
    }

    if runs.is_empty() {
        return None;
    }

    // Detectar heading por namedStyleType
    let style_name = para.get("paragraphStyle")
        .and_then(|s| s.get("namedStyleType"))
        .and_then(|v| v.as_str());

    if let Some(name) = style_name {
        if let Some(level) = heading_level_from_style(name) {
            let text: String = runs.iter().map(|r| r.text.as_str()).collect();
            return Some(Element::Heading {
                level,
                text: text.trim().to_string(),
            });
        }
    }

    // Detectar bullets
    if para.get("bullet").is_some() {
        // Es parte de una lista — lo tratamos como párrafo normal
        // (las listas se agrupan después)
    }

    Some(Element::Paragraph { runs })
}

/// Parsear una tabla de Google Docs.
#[cfg(feature = "google")]
fn parse_table(table: &serde_json::Value) -> Option<Element> {
    let mut rows: Vec<Vec<String>> = Vec::new();

    if let Some(table_rows) = table.get("tableRows") {
        if let Some(items) = table_rows.as_array() {
            for row_item in items {
                let mut row: Vec<String> = Vec::new();
                if let Some(cells) = row_item.get("tableCells") {
                    if let Some(cell_array) = cells.as_array() {
                        for cell in cell_array {
                            let cell_text = extract_cell_text(cell);
                            row.push(cell_text);
                        }
                    }
                }
                if !row.is_empty() {
                    rows.push(row);
                }
            }
        }
    }

    if rows.is_empty() {
        return None;
    }

    Some(Element::Table { rows })
}

/// Extraer texto de una celda de tabla.
#[cfg(feature = "google")]
fn extract_cell_text(cell: &serde_json::Value) -> String {
    let mut text = String::new();
    if let Some(content) = cell.get("content") {
        if let Some(items) = content.as_array() {
            for item in items {
                if let Some(para) = item.get("paragraph") {
                    if let Some(elements) = para.get("elements") {
                        if let Some(el_array) = elements.as_array() {
                            for el in el_array {
                                if let Some(text_run) = el.get("textRun") {
                                    if let Some(content) = text_run.get("content") {
                                        text.push_str(content.as_str().unwrap_or(""));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    text.trim().to_string()
}

/// Convertir namedStyleType a nivel de heading.
fn heading_level_from_style(name: &str) -> Option<u8> {
    match name {
        "TITLE" => Some(1),
        "SUBTITLE" => Some(2),
        "HEADING_1" => Some(1),
        "HEADING_2" => Some(2),
        "HEADING_3" => Some(3),
        "HEADING_4" => Some(4),
        "HEADING_5" => Some(5),
        "HEADING_6" => Some(6),
        _ => None,
    }
}

/// Convertir RGB color de Google Docs a hex.
fn rgb_color_to_hex(color: &serde_json::Value) -> Option<String> {
    let r = color.get("rgbColor")?.get("red")?.as_f64()?;
    let g = color.get("rgbColor")?.get("green")?.as_f64()?;
    let b = color.get("rgbColor")?.get("blue")?.as_f64()?;

    let ri = (r * 255.0) as u8;
    let gi = (g * 255.0) as u8;
    let bi = (b * 255.0) as u8;

    Some(format!("{ri:02X}{gi:02X}{bi:02X}"))
}

// ── Google Docs Writer ────────────────────────────────────────────────────────

/// Escribir contenido de un OxtIR a un Google Doc existente.
///
/// Estrategia: borra todo el contenido del doc y lo reemplaza con el IR.
#[cfg(feature = "google")]
pub fn write_doc(document_id: &str, ir: &crate::ir::OxtIR) -> Result<()> {
    #[allow(unused_imports)]
use crate::ir::Element;

    let tokens = load_tokens()?;
    let url = format!(
        "https://docs.googleapis.com/v1/documents/{document_id}:batchUpdate"
    );

    // Construir requests de batchUpdate
    let mut requests: Vec<serde_json::Value> = Vec::new();

    // 1. Borrar todo el contenido existente
    requests.push(serde_json::json!({
        "deleteContentRange": {
            "range": {
                "startIndex": 1,
                "endIndex": get_doc_end_index(document_id)?
            }
        }
    }));

    // 2. Insertar contenido desde OxtIR
    let mut text = String::new();
    for section in &ir.sections {
        if let Some(ref title) = section.title {
            text.push_str(&title);
            text.push('\n');
        }
        for element in &section.elements {
            match element {
                Element::Heading { level: _, text: t } => {
                    text.push_str(t);
                    text.push('\n');
                }
                Element::Paragraph { runs } => {
                    for run in runs {
                        text.push_str(&run.text);
                    }
                    text.push('\n');
                }
                Element::List { items, .. } => {
                    for item in items {
                        text.push_str(&format!("• {item}\n"));
                    }
                }
                Element::Table { rows } => {
                    for row in rows {
                        text.push_str(&row.join("\t"));
                        text.push('\n');
                    }
                    text.push('\n');
                }
                _ => {}
            }
        }
    }

    // Google Docs usa índices: el documento empieza en 1
    requests.push(serde_json::json!({
        "insertText": {
            "location": { "index": 1 },
            "text": text
        }
    }));

    let body = serde_json::json!({ "requests": requests });

    let _resp: serde_json::Value = ureq::post(&url)
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    Ok(())
}

/// Obtener el último índice del documento (para borrar contenido).
#[cfg(feature = "google")]
fn get_doc_end_index(document_id: &str) -> Result<i64> {
    let tokens = load_tokens()?;
    let url = format!(
        "https://docs.googleapis.com/v1/documents/{document_id}"
    );

    let resp: serde_json::Value = ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .call()
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    // El índice final es el último content[].endIndex
    let end_index = resp.pointer("/body/content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.last())
        .and_then(|last| last.get("endIndex"))
        .and_then(|v| v.as_i64())
        .unwrap_or(2);

    Ok(end_index)
}

/// Crear un nuevo Google Doc en blanco.
#[cfg(feature = "google")]
pub fn create_doc(title: &str) -> Result<String> {
    let tokens = load_tokens()?;

    let body = serde_json::json!({
        "title": title,
    });

    let resp: serde_json::Value = ureq::post("https://docs.googleapis.com/v1/documents")
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    let doc_id = resp.get("documentId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GoogleError::Api("No documentId en respuesta".into()))?;

    Ok(doc_id.to_string())
}


// ── Google Sheets Reader ─────────────────────────────────────────────────────

/// Leer un Google Sheet y devolverlo como OxtIR.
/// Cada hoja (sheet) del documento se convierte en una Section con un Element::Table.
#[cfg(feature = "google")]
pub fn read_sheet(spreadsheet_id: &str) -> Result<crate::ir::OxtIR> {
    use crate::ir::{Element, Metadata, Section};

    let tokens = load_tokens()?;
    let url = format!(
        "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}?includeGridData=true"
    );

    let resp: serde_json::Value = ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .call()
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    let title = resp.get("properties")
        .and_then(|p| p.get("title"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut sections: Vec<Section> = Vec::new();

    if let Some(sheets) = resp.get("sheets").and_then(|s| s.as_array()) {
        for sheet_entry in sheets {
            let sheet_title = sheet_entry
                .pointer("/properties/title")
                .and_then(|v| v.as_str())
                .unwrap_or("Sheet")
                .to_string();

            let mut rows: Vec<Vec<String>> = Vec::new();

            if let Some(grid_data) = sheet_entry.get("data").and_then(|d| d.as_array()) {
                for data in grid_data {
                    if let Some(row_data) = data.get("rowData").and_then(|r| r.as_array()) {
                        for row_entry in row_data {
                            let mut row: Vec<String> = Vec::new();
                            if let Some(values) = row_entry.get("values").and_then(|v| v.as_array()) {
                                for cell in values {
                                    let cell_text = cell
                                        .pointer("/effectiveValue/stringValue")
                                        .or_else(|| cell.pointer("/effectiveValue/numberValue"))
                                        .and_then(|v| {
                                            if let Some(s) = v.as_str() {
                                                Some(s.to_string())
                                            } else if let Some(n) = v.as_f64() {
                                                if n == n.floor() {
                                                    Some(format!("{}", n as i64))
                                                } else {
                                                    Some(format!("{n}"))
                                                }
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_default();
                                    row.push(cell_text);
                                }
                            }
                            // Skip empty rows
                            if !row.is_empty() && row.iter().any(|c| !c.is_empty()) {
                                rows.push(row);
                            }
                        }
                    }
                }
            }

            let elements = if rows.is_empty() {
                vec![]
            } else {
                vec![Element::Table { rows }]
            };

            sections.push(Section {
                title: Some(sheet_title),
                elements,
            });
        }
    }

    let ir = crate::ir::OxtIR {
        metadata: Metadata {
            title,
            subject: None,
            creator: None,
            page_count: None,
            word_count: None,
        },
        sections,
    };

    Ok(ir)
}

/// Crear un nuevo Google Sheet en blanco.
#[cfg(feature = "google")]
pub fn create_sheet(title: &str) -> Result<String> {
    let tokens = load_tokens()?;

    let body = serde_json::json!({
        "properties": { "title": title },
    });

    let resp: serde_json::Value = ureq::post("https://sheets.googleapis.com/v4/spreadsheets")
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    let sheet_id = resp.get("spreadsheetId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GoogleError::Api("No spreadsheetId en respuesta".into()))?;

    Ok(sheet_id.to_string())
}

/// Escribir contenido de un OxtIR a un Google Sheet existente.
#[cfg(feature = "google")]
pub fn write_sheet(spreadsheet_id: &str, ir: &crate::ir::OxtIR) -> Result<()> {
    #[allow(unused_imports)]
use crate::ir::Element;

    let tokens = load_tokens()?;

    let mut requests: Vec<serde_json::Value> = Vec::new();

    // Google Sheets API usa updateCells para escribir datos
    for (section_idx, section) in ir.sections.iter().enumerate() {
        for element in &section.elements {
            if let Element::Table { rows } = element {
                // Construir filas para updateCells
                let mut grid_rows: Vec<serde_json::Value> = Vec::new();
                for row in rows {
                    let mut values: Vec<serde_json::Value> = Vec::new();
                    for cell in row {
                        values.push(serde_json::json!({
                            "userEnteredValue": {
                                "stringValue": cell
                            }
                        }));
                    }
                    grid_rows.push(serde_json::json!({
                        "values": values
                    }));
                }

                requests.push(serde_json::json!({
                    "updateCells": {
                        "rows": grid_rows,
                        "fields": "userEnteredValue",
                        "start": {
                            "sheetId": section_idx,
                            "rowIndex": 0,
                            "columnIndex": 0
                        }
                    }
                }));
            }
        }
    }

    if requests.is_empty() {
        return Ok(());
    }

    let body = serde_json::json!({ "requests": requests });

    let url = format!(
        "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}:batchUpdate"
    );

    let _resp: serde_json::Value = ureq::post(&url)
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    Ok(())
}


// ── Google Slides Reader ──────────────────────────────────────────────────────

/// Leer una presentación de Google Slides y devolverla como OxtIR.
/// Cada slide = una Section, con párrafos/headings/listas como elements.
#[cfg(feature = "google")]
pub fn read_slides(presentation_id: &str) -> Result<crate::ir::OxtIR> {
    use crate::ir::{Element, Metadata, Run, Section};

    let tokens = load_tokens()?;
    let url = format!(
        "https://slides.googleapis.com/v1/presentations/{presentation_id}"
    );

    let resp: serde_json::Value = ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .call()
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    let title = resp.get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut sections: Vec<Section> = Vec::new();

    if let Some(slides) = resp.get("slides").and_then(|s| s.as_array()) {
        for slide_entry in slides {
            let mut elements: Vec<Element> = Vec::new();

            if let Some(page_elements) = slide_entry.get("pageElements")
                .and_then(|p| p.as_array())
            {
                for pe in page_elements {
                    // Extraer texto de shapes, text boxes, etc.
                    if let Some(shape) = pe.get("shape") {
                        if let Some(text) = shape.get("text") {
                            if let Some(text_elements) = text.get("textElements")
                                .and_then(|t| t.as_array())
                            {
                                let mut runs: Vec<Run> = Vec::new();
                                let mut is_heading = false;

                                for te in text_elements {
                                    if let Some(para_style) = te.get("paragraphStyle") {
                                        // Detectar si es heading por spacing
                                        if let Some(spacing) = para_style.get("spaceAbove") {
                                            if let Some(pts) = spacing.get("magnitude") {
                                                if pts.as_f64().unwrap_or(0.0) > 10.0 {
                                                    is_heading = true;
                                                }
                                            }
                                        }
                                        if let Some(named_style) = para_style.get("namedStyleType") {
                                            if let Some(name) = named_style.as_str() {
                                                if name.starts_with("HEADING_") || name == "TITLE" {
                                                    is_heading = true;
                                                }
                                            }
                                        }
                                    }

                                    if let Some(text_run) = te.get("textRun") {
                                        let content = text_run.get("content")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();

                                        if content.trim().is_empty() {
                                            continue;
                                        }

                                        let mut run = Run::plain(&content);

                                        if let Some(style) = text_run.get("style") {
                                            run.bold = style.get("bold").and_then(|v| v.as_bool());
                                            run.italic = style.get("italic").and_then(|v| v.as_bool());
                                            run.underline = style.get("underline").and_then(|v| v.as_bool());
                                            if let Some(fg) = style.get("foregroundColor") {
                                                if let Some(color) = fg.get("color") {
                                                    run.color = rgb_color_to_hex(color);
                                                }
                                            }
                                            if let Some(font_size) = style.get("fontSize") {
                                                if let Some(pts) = font_size.get("magnitude") {
                                                    if let Some(sz) = pts.as_f64() {
                                                        run.font_size = Some(sz as f32);
                                                    }
                                                }
                                            }
                                        }

                                        runs.push(run);
                                    }
                                }

                                if !runs.is_empty() {
                                    if is_heading {
                                        let text: String = runs.iter().map(|r| r.text.as_str()).collect();
                                        elements.push(Element::Heading {
                                            level: 2,
                                            text: text.trim().to_string(),
                                        });
                                    } else {
                                        elements.push(Element::Paragraph { runs });
                                    }
                                }
                            }
                        }
                    }

                    // Tablas en slides
                    if let Some(table) = pe.get("table") {
                        if let Some(rows) = table.get("tableRows").and_then(|r| r.as_array()) {
                            let mut table_rows: Vec<Vec<String>> = Vec::new();
                            for row_entry in rows {
                                let mut row: Vec<String> = Vec::new();
                                if let Some(cells) = row_entry.get("tableCells")
                                    .and_then(|c| c.as_array())
                                {
                                    for cell in cells {
                                        let cell_text = cell
                                            .pointer("/text/textElements/0/textRun/content")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        row.push(cell_text.trim().to_string());
                                    }
                                }
                                if !row.is_empty() {
                                    table_rows.push(row);
                                }
                            }
                            if !table_rows.is_empty() {
                                elements.push(Element::Table { rows: table_rows });
                            }
                        }
                    }
                }
            }

            sections.push(Section {
                title: None,
                elements,
            });
        }
    }

    let ir = crate::ir::OxtIR {
        metadata: Metadata {
            title,
            subject: None,
            creator: None,
            page_count: None,
            word_count: None,
        },
        sections,
    };

    Ok(ir)
}

/// Crear una presentación en blanco en Google Slides.
#[cfg(feature = "google")]
pub fn create_slides(title: &str) -> Result<String> {
    let tokens = load_tokens()?;

    let body = serde_json::json!({
        "title": title,
    });

    let resp: serde_json::Value = ureq::post("https://slides.googleapis.com/v1/presentations")
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    let pres_id = resp.get("presentationId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GoogleError::Api("No presentationId en respuesta".into()))?;

    Ok(pres_id.to_string())
}

/// Escribir contenido de un OxtIR a una presentación de Google Slides.
#[cfg(feature = "google")]
pub fn write_slides(presentation_id: &str, ir: &crate::ir::OxtIR) -> Result<()> {
    #[allow(unused_imports)]
use crate::ir::Element;

    let tokens = load_tokens()?;
    let mut requests: Vec<serde_json::Value> = Vec::new();

    // Primero: borrar slides existentes (excepto el primero)
    requests.push(serde_json::json!({
        "deleteObject": {
            "objectId": "p"  // placeholder
        }
    }));

    // Por cada sección, crear un slide
    for (section_idx, section) in ir.sections.iter().enumerate() {
        let slide_id = format!("slide_{section_idx}");

        // Crear slide
        requests.push(serde_json::json!({
            "createSlide": {
                "objectId": slide_id,
                "slideLayoutReference": {
                    "predefinedLayout": "BLANK"
                },
                "placeholderIdMappings": []
            }
        }));

        // Agregar texto como shapes
        for element in &section.elements {
            match element {
                Element::Heading { text, .. } => {
                    let shape_id = format!("{slide_id}_title");
                    requests.push(serde_json::json!({
                        "createShape": {
                            "objectId": shape_id,
                            "shapeType": "TEXT_BOX",
                            "elementProperties": {
                                "pageObjectId": slide_id,
                                "size": {
                                    "width": { "magnitude": 300, "unit": "PT" },
                                    "height": { "magnitude": 50, "unit": "PT" }
                                },
                                "transform": {
                                    "scaleX": 1, "scaleY": 1,
                                    "translateX": 50, "translateY": 50.0 + (section_idx * 300) as f64,
                                    "unit": "PT"
                                }
                            }
                        }
                    }));
                    requests.push(serde_json::json!({
                        "insertText": {
                            "objectId": shape_id,
                            "text": text
                        }
                    }));
                }
                Element::Paragraph { runs } => {
                    let shape_id = format!("{slide_id}_p_{}", section_idx * 100);
                    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
                    requests.push(serde_json::json!({
                        "createShape": {
                            "objectId": shape_id,
                            "shapeType": "TEXT_BOX",
                            "elementProperties": {
                                "pageObjectId": slide_id,
                                "size": {
                                    "width": { "magnitude": 300, "unit": "PT" },
                                    "height": { "magnitude": 30, "unit": "PT" }
                                },
                                "transform": {
                                    "scaleX": 1, "scaleY": 1,
                                    "translateX": 50, "translateY": 120.0 + (section_idx * 50) as f64,
                                    "unit": "PT"
                                }
                            }
                        }
                    }));
                    requests.push(serde_json::json!({
                        "insertText": {
                            "objectId": shape_id,
                            "text": text
                        }
                    }));
                }
                Element::List { items, .. } => {
                    for (item_idx, item) in items.iter().enumerate() {
                        let shape_id = format!("{slide_id}_li_{item_idx}");
                        requests.push(serde_json::json!({
                            "createShape": {
                                "objectId": shape_id,
                                "shapeType": "TEXT_BOX",
                                "elementProperties": {
                                    "pageObjectId": slide_id,
                                    "size": {
                                        "width": { "magnitude": 300, "unit": "PT" },
                                        "height": { "magnitude": 30, "unit": "PT" }
                                    },
                                    "transform": {
                                        "scaleX": 1, "scaleY": 1,
                                        "translateX": 70, "translateY": 120.0 + (item_idx * 30) as f64,
                                        "unit": "PT"
                                    }
                                }
                            }
                        }));
                        requests.push(serde_json::json!({
                            "insertText": {
                                "objectId": shape_id,
                                "text": format!("• {item}")
                            }
                        }));
                    }
                }
                _ => {}
            }
        }
    }

    if requests.is_empty() {
        return Ok(());
    }

    // Eliminar el primer request (deleteObject placeholder) y reemplazar
    // con lógica real: obtener slides existentes y borrarlos
    requests.remove(0);

    let body = serde_json::json!({ "requests": requests });

    let url = format!(
        "https://slides.googleapis.com/v1/presentations/{presentation_id}:batchUpdate"
    );

    let _resp: serde_json::Value = ureq::post(&url)
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    Ok(())
}

// ── Google Drive ────────────────────────────────────────────────────────────

/// Listar archivos de Google Drive.
/// Devuelve JSON con los archivos (id, name, mimeType, modifiedTime).
#[cfg(feature = "google")]
pub fn list_drive_files(query: Option<&str>) -> Result<serde_json::Value> {
    let tokens = load_tokens()?;
    let mut url = format!(
        "https://www.googleapis.com/drive/v3/files?fields=files(id,name,mimeType,modifiedTime,size)"
    );
    if let Some(q) = query {
        url.push_str(&format!("&q={}", urlencoding(q)));
    }

    let resp: serde_json::Value = ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .call()
        .map_err(|e| GoogleError::Http(e.to_string()))?
        .body_mut().read_json::<serde_json::Value>()
            .map_err(|e| GoogleError::Http(e.to_string()))?;

    Ok(resp)
}

/// Descargar un archivo de Google Drive.
/// Guarda el contenido en la ruta especificada.
#[cfg(feature = "google")]
pub fn download_drive_file(file_id: &str, output: &str) -> Result<()> {
    let tokens = load_tokens()?;
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{file_id}?alt=media"
    );

    let mut response = ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", tokens.access_token))
        .call()
        .map_err(|e| GoogleError::Http(e.to_string()))?;

    let body = response.body_mut().read_to_vec()
        .map_err(|e| GoogleError::Http(e.to_string()))?;

    std::fs::write(output, &body)
        .map_err(|e| GoogleError::Io(e))?;

    println!("Descargado: {} ({} bytes)", output, body.len());
    Ok(())
}

/// URL-encode simple para parámetros de query.
/// URL-encode para query params de API (espacios como %20).
fn urlencoding(s: &str) -> String {
    s.chars().map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '-' | '.' | '~' => c.to_string(),
        ' ' => "%20".to_string(),
        _ => format!("%{:02X}", c as u8),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading_level_from_style() {
        assert_eq!(heading_level_from_style("TITLE"), Some(1));
        assert_eq!(heading_level_from_style("HEADING_1"), Some(1));
        assert_eq!(heading_level_from_style("HEADING_3"), Some(3));
        assert_eq!(heading_level_from_style("NORMAL_TEXT"), None);
        assert_eq!(heading_level_from_style("SUBTITLE"), Some(2));
    }

    #[test]
    fn test_rgb_color_to_hex() {
        let color = serde_json::json!({
            "rgbColor": { "red": 1.0, "green": 0.0, "blue": 0.0 }
        });
        assert_eq!(rgb_color_to_hex(&color), Some("FF0000".into()));

        let color = serde_json::json!({
            "rgbColor": { "red": 0.5, "green": 0.5, "blue": 0.5 }
        });
        assert_eq!(rgb_color_to_hex(&color), Some("7F7F7F".into()));
    }

    #[test]
    fn test_doc_to_ir_basic() {
        let doc_json = serde_json::json!({
            "title": "Test Doc",
            "body": {
                "content": [
                    {
                        "paragraph": {
                            "elements": [
                                {
                                    "textRun": {
                                        "content": "Hello World",
                                        "textStyle": { "bold": true }
                                    }
                                }
                            ],
                            "paragraphStyle": {
                                "namedStyleType": "HEADING_1"
                            }
                        }
                    },
                    {
                        "paragraph": {
                            "elements": [
                                {
                                    "textRun": {
                                        "content": "Normal paragraph"
                                    }
                                }
                            ],
                            "paragraphStyle": {
                                "namedStyleType": "NORMAL_TEXT"
                            }
                        }
                    }
                ]
            }
        });

        #[cfg(feature = "google")]
        {
            let ir = doc_to_ir(&doc_json).unwrap();
            assert_eq!(ir.sections.len(), 1);
            assert_eq!(ir.metadata.title.as_deref(), Some("Test Doc"));
            assert_eq!(ir.sections[0].elements.len(), 2);

            // Primer elemento: heading
            match &ir.sections[0].elements[0] {
                crate::ir::Element::Heading { level, text } => {
                    assert_eq!(*level, 1);
                    assert_eq!(text, "Hello World");
                }
                _ => panic!("Expected Heading"),
            }

            // Segundo elemento: paragraph
            match &ir.sections[0].elements[1] {
                crate::ir::Element::Paragraph { runs } => {
                    assert!(!runs.is_empty(), "debe tener al menos un run");
                    assert_eq!(runs[0].text, "Normal paragraph");
                }
                _ => panic!("Expected Paragraph"),
            }
        }

        // Sin feature google, solo probar que los helpers funcionan
        #[cfg(not(feature = "google"))]
        {
            assert_eq!(heading_level_from_style("HEADING_1"), Some(1));
        }
    }
}
