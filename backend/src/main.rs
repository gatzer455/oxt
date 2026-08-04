//! oxt — CLI para documentos de oficina, contrato v1.
//!
//! Especificación canónica: `docs/cli.md`. Un verbo, cualquier origen:
//! `read`, `edit`, `update`, … aceptan path local o documento de Google.

use clap::{CommandFactory, Parser, Subcommand};

use oxt_backend::ir::OxtIR;
use oxt_backend::origin::{GoogleKind, Origin, resolve_origin};
use oxt_backend::Error as BackendError;

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "oxt", about = "Documentos de oficina para LLMs", version)]
struct Cli {
    /// Salida estructurada JSON (contrato v1)
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Leer un documento (local o Google) y mostrarlo
    Read {
        /// Path, URL de Google o ID
        origin: String,
        /// Formato de salida: text, markdown, ir, offset-map
        #[arg(long, default_value = "markdown")]
        format: String,
    },

    /// Mostrar información del documento
    Info { origin: String },

    /// Métricas del documento
    Stats {
        origin: String,
        /// Desglose por sección
        #[arg(long)]
        per_section: bool,
    },

    /// Buscar un patrón (regex por defecto) con posición exacta
    Grep {
        /// Patrón (regex) o texto literal con --literal
        pattern: String,
        origin: String,
        /// Tratar el patrón como texto literal
        #[arg(long)]
        literal: bool,
        /// Ignorar mayúsculas/minúsculas
        #[arg(short = 'i', long)]
        ignore_case: bool,
    },

    /// Comparar dos documentos
    Diff { a: String, b: String },

    /// Reemplazar texto en un documento
    Edit {
        origin: String,
        /// Texto a reemplazar
        #[arg(long)]
        old: String,
        /// Texto nuevo
        #[arg(long)]
        new: String,
    },

    /// Reemplazar el contenido completo con un IR
    Update {
        origin: String,
        /// Archivo JSON con el IR, o `-` para stdin
        #[arg(long)]
        from: String,
    },

    /// Convertir entre formatos (destino local)
    Convert { origin: String, dest: String },

    /// Crear un documento (local con <path>, o Google con --doc/--sheet/--slides)
    Create {
        /// Path local de salida (ej: reporte.docx)
        path: Option<String>,
        /// Crear un Google Doc con este título
        #[arg(long)]
        doc: Option<String>,
        /// Crear un Google Sheet con este título
        #[arg(long)]
        sheet: Option<String>,
        /// Crear una presentación de Google Slides con este título
        #[arg(long)]
        slides: Option<String>,
        /// Archivo JSON con el IR, o `-` para stdin (opcional: crea vacío)
        #[arg(long)]
        from: Option<String>,
    },

    /// Extraer imágenes a un directorio
    Media {
        origin: String,
        /// Directorio de salida
        #[arg(long, default_value = "media/")]
        output: String,
    },

    /// Listar archivos de Google Drive
    List {
        /// Filtro de Drive (ej: "name contains 'reporte'")
        #[arg(long)]
        query: Option<String>,
    },

    /// Descargar un archivo de Google Drive (id o URL)
    Download {
        id: String,
        /// Ruta de salida
        #[arg(long)]
        output: String,
    },

    /// Esquema JSON de la CLI (siempre JSON)
    Schema,

    /// Autenticación con Google Workspace
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Autenticar con el navegador (o guardar un refresh token con --token)
    Login {
        /// Client ID de GCP (default: credenciales embebidas)
        #[arg(long)]
        client_id: Option<String>,
        /// Client Secret de GCP (default: credenciales embebidas)
        #[arg(long)]
        client_secret: Option<String>,
        /// Guardar un refresh token obtenido out-of-band (headless/CI)
        #[arg(long)]
        token: Option<String>,
    },
    /// Borrar los tokens guardados
    Logout,
    /// Estado de autenticación (exit 1 si no autenticado)
    Status,
}

// ── Errores: envelope JSON en stderr + exit codes tipificados ─────────────────

/// Emitir error JSON en stderr y salir con el código del kind.
fn fail(kind: &str, message: impl std::fmt::Display) -> ! {
    fail_hint(kind, message, None)
}

fn fail_hint(kind: &str, message: impl std::fmt::Display, hint: Option<&str>) -> ! {
    let mut v = serde_json::json!({ "kind": kind, "message": message.to_string() });
    if let Some(h) = hint {
        v["hint"] = h.into();
    }
    eprintln!("{v}");
    std::process::exit(exit_code(kind));
}

fn exit_code(kind: &str) -> i32 {
    match kind {
        "usage" => 2,
        "io_error" => 3,
        "unsupported_format" | "parse_error" => 4,
        "invalid_ir" => 5,
        "auth_error" => 6,
        "api_error" => 7,
        "edit_error" => 8,
        _ => 10, // internal_error
    }
}

/// Mapear un error del backend al envelope del contrato.
fn map_backend(e: BackendError) -> ! {
    match e {
        BackendError::Io(_) => fail("io_error", e),
        BackendError::UnsupportedFormat(_) => fail("unsupported_format", e),
        BackendError::Docx(_)
        | BackendError::Xlsx(_)
        | BackendError::Pptx(_)
        | BackendError::Odf(_)
        | BackendError::Legacy(_)
        | BackendError::Opc(_) => fail("parse_error", e),
        BackendError::Create(_) => fail("io_error", e),
        BackendError::Edit(_) => fail("edit_error", e),
        BackendError::Roundtrip(_) => fail("io_error", e),
        BackendError::Google(g) => {
            use oxt_backend::google::GoogleError;
            match g {
                GoogleError::AuthRequired | GoogleError::AuthFailed(_) => fail("auth_error", g),
                GoogleError::Http(_) | GoogleError::Api(_) => fail("api_error", g),
                GoogleError::Io(_) => fail("io_error", g),
                GoogleError::Json(_) => fail("parse_error", g),
                GoogleError::Other(_) => fail("internal_error", g),
            }
        }
        BackendError::Other(_) => fail("internal_error", e),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_stdin() -> Vec<u8> {
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf).unwrap_or_default();
    buf
}

fn resolve(input: &str) -> Origin {
    resolve_origin(input).unwrap_or_else(|e| {
        fail_hint(
            "usage",
            e.0,
            Some("usá un path existente, una URL de Google o un ID de Google"),
        )
    })
}

/// Abrir un documento local (o stdin) a OxtIR. Devuelve (ir, label de formato).
fn open_local(origin: &Origin) -> (OxtIR, String) {
    match origin {
        Origin::Local(p) => {
            let doc = oxt_backend::Document::open(p).unwrap_or_else(|e| map_backend(e));
            let label = doc.format().to_string();
            (doc.into_ir(), label)
        }
        Origin::Stdin => {
            let doc = oxt_backend::open_from_bytes(&read_stdin()).unwrap_or_else(|e| map_backend(e));
            let label = doc.format().to_string();
            (doc.into_ir(), label)
        }
        Origin::Google { .. } => unreachable!("google se maneja en open_any"),
    }
}

/// Leer un IR de Google por id (resolviendo el kind si es necesario).
#[cfg(feature = "google")]
fn google_ir(id: &str, kind: Option<GoogleKind>) -> OxtIR {
    use oxt_backend::google;
    let kind = match kind {
        Some(k) => k,
        None => {
            let (mime, _name) =
                google::get_file_metadata(id).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e));
            GoogleKind::from_mime(&mime).unwrap_or_else(|| {
                fail("unsupported_format", format!("no es un documento de Google soportado (doc/sheet/slides): {mime}"))
            })
        }
    };
    match kind {
        GoogleKind::Doc => google::read_doc(id).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
        GoogleKind::Sheet => google::read_sheet(id).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
        GoogleKind::Slides => google::read_slides(id).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
    }
}

#[cfg(not(feature = "google"))]
fn google_ir(_id: &str, _kind: Option<GoogleKind>) -> OxtIR {
    fail("api_error", "Google no habilitado — compile con --features google")
}

/// Abrir cualquier origen a OxtIR. Devuelve (ir, label de formato).
fn open_any(origin: &Origin) -> (OxtIR, String) {
    match origin {
        Origin::Local(_) | Origin::Stdin => open_local(origin),
        Origin::Google { id, kind } => (google_ir(id, *kind), "google".into()),
    }
}

fn parse_ir(from: &str) -> OxtIR {
    let text = if from == "-" {
        String::from_utf8_lossy(&read_stdin()).into_owned()
    } else {
        std::fs::read_to_string(from)
            .unwrap_or_else(|e| fail("io_error", format!("no se pudo leer {from}: {e}")))
    };
    serde_json::from_str(&text)
        .unwrap_or_else(|e| fail("invalid_ir", format!("JSON inválido en {from}: {e}")))
}

fn print_json(v: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap());
}

// ── Verbos ────────────────────────────────────────────────────────────────────

fn cmd_read(origin: &str, format: &str, json: bool) {
    let (ir, label) = open_any(&resolve(origin));
    let data = match format {
        "text" => serde_json::Value::String(ir.plain_text()),
        "markdown" => serde_json::Value::String(ir.to_markdown()),
        "ir" => serde_json::to_value(&ir).unwrap(),
        "offset-map" => serde_json::to_value(ir.to_offset_map(&label)).unwrap(),
        _ => fail("usage", format!("formato no válido: {format} (usa: text, markdown, ir, offset-map)")),
    };
    if json {
        print_json(&serde_json::json!({ "format": format, "data": data }));
    } else if format == "ir" || format == "offset-map" {
        print_json(&data);
    } else {
        println!("{}", data.as_str().unwrap());
    }
}

fn cmd_info(origin: &str, json: bool) {
    let (ir, label) = open_any(&resolve(origin));
    let element_count: usize = ir.sections.iter().map(|s| s.elements.len()).sum();
    if json {
        print_json(&serde_json::json!({
            "path": origin,
            "format": label,
            "sections": ir.sections.len(),
            "elements": element_count,
            "title": ir.metadata.title,
        }));
    } else {
        println!("Archivo: {origin}");
        println!("Formato: {label}");
        println!("Secciones: {}", ir.sections.len());
        println!("Elementos: {element_count}");
        if let Some(ref title) = ir.metadata.title {
            println!("Título: {title}");
        }
    }
}

fn cmd_stats(origin: &str, per_section: bool, json: bool) {
    let (ir, _) = open_any(&resolve(origin));
    let st = ir.stats(per_section);
    if json {
        print_json(&serde_json::to_value(&st).unwrap());
        return;
    }
    println!("Secciones: {}", st.sections);
    for (kind, n) in &st.elements {
        println!("Elementos {kind}: {n}");
    }
    println!("Párrafos: {}", st.paragraphs);
    println!("Palabras: {}", st.words);
    println!("Caracteres: {}", st.characters);
    println!("Tablas: {} (filas {}, celdas {})", st.tables, st.table_rows, st.cells);
    println!("Items de lista: {}", st.list_items);
    println!("Imágenes: {}", st.images);
    for (level, n) in &st.headings {
        println!("Headings nivel {level}: {n}");
    }
    println!("Hipervínculos: {}", st.hyperlinks);
    if per_section {
        for sec in &st.per_section {
            let title = sec.title.as_deref().unwrap_or("(sin título)");
            println!("  [{title}] elementos {}, palabras {}", sec.elements, sec.words);
        }
    }
}

fn cmd_grep(pattern: &str, origin: &str, literal: bool, ignore_case: bool, json: bool) {
    let (ir, _) = open_any(&resolve(origin));
    let matches = ir.search(pattern, literal, ignore_case).unwrap_or_else(|e| {
        fail("usage", format!("patrón inválido: {e}"))
    });
    if json {
        print_json(&serde_json::json!({ "matches": matches }));
    } else {
        for m in &matches {
            println!("{}:{}:{}", origin, m.offset, m.text);
        }
    }
    if matches.is_empty() {
        std::process::exit(1); // outcome: sin matches
    }
}

fn cmd_diff(a: &str, b: &str, json: bool) {
    let (ira, _) = open_any(&resolve(a));
    let (irb, _) = open_any(&resolve(b));
    let report = ira.diff(&irb);
    if json {
        print_json(&serde_json::to_value(&report).unwrap());
    } else {
        for c in &report.changes {
            match (c.old.as_deref(), c.new.as_deref()) {
                (Some(o), Some(n)) => println!("{}: - {o} / + {n}", c.path),
                (Some(o), None) => println!("{}: - {o}", c.path),
                (None, Some(n)) => println!("{}: + {n}", c.path),
                _ => println!("{}: {}", c.path, c.kind),
            }
        }
        if report.equal {
            println!("Documentos iguales");
        }
    }
    if !report.equal {
        std::process::exit(1); // outcome: hay diferencias
    }
}

fn cmd_edit(origin: &str, old: &str, new: &str, json: bool) {
    let resolved = resolve(origin);
    let result = match &resolved {
        Origin::Local(p) => oxt_backend::edit::replace_text(p, old, new)
            .map_err(BackendError::from)
            .unwrap_or_else(|e| map_backend(e)),
        Origin::Stdin => fail("usage", "edit no soporta stdin: pasá un path de archivo"),
        Origin::Google { id, kind } => {
            #[cfg(feature = "google")]
            {
                let mut ir = google_ir(id, *kind);
                let replacements = oxt_backend::edit::replace_in_ir(&mut ir, old, new);
                if replacements > 0 {
                    write_google(id, *kind, &ir).unwrap_or_else(|e| map_backend(e));
                }
                oxt_backend::edit::EditResult {
                    path: origin.to_string(),
                    replacements,
                    affected_parts: vec!["google".into()],
                }
            }
            #[cfg(not(feature = "google"))]
            {
                let _ = (id, kind);
                fail("api_error", "Google no habilitado — compile con --features google")
            }
        }
    };
    if json {
        print_json(&serde_json::json!({
            "replacements": result.replacements,
            "changed": result.replacements > 0,
            "affected_parts": result.affected_parts,
        }));
    } else {
        println!("Reemplazos: {}", result.replacements);
        if !result.affected_parts.is_empty() {
            for part in &result.affected_parts {
                if part.starts_with("convertido") || part == "google" {
                    eprintln!("⚠️  {part}");
                }
            }
        }
    }
}

fn cmd_update(origin: &str, from: &str, json: bool) {
    let ir = parse_ir(from);
    let resolved = resolve(origin);
    match &resolved {
        Origin::Local(p) => {
            let r = oxt_backend::edit::update_from_ir(p, &ir)
                .map_err(BackendError::from)
                .unwrap_or_else(|e| map_backend(e));
            if json {
                print_json(&serde_json::to_value(&r).unwrap());
            } else {
                println!("Actualizado: {}", r.path);
                if let Some(conv) = &r.converted_from {
                    eprintln!("⚠️  convertido de .{conv} a {}", r.format);
                }
            }
        }
        Origin::Stdin => fail("usage", "update no soporta stdin como origen: pasá un path de archivo"),
        Origin::Google { id, kind } => {
            #[cfg(feature = "google")]
            {
                write_google(id, *kind, &ir).unwrap_or_else(|e| map_backend(e));
                if json {
                    print_json(&serde_json::json!({ "id": id, "url": google_url(kind.unwrap_or(GoogleKind::Doc), id) }));
                } else {
                    println!("Actualizado: {id}");
                }
            }
            #[cfg(not(feature = "google"))]
            {
                let _ = (id, kind);
                fail("api_error", "Google no habilitado — compile con --features google")
            }
        }
    }
}

fn cmd_convert(origin: &str, dest: &str, json: bool) {
    let (ir, _) = open_any(&resolve(origin));
    oxt_backend::create::create_from_ir(dest, &ir)
        .map_err(BackendError::from)
        .unwrap_or_else(|e| map_backend(e));
    let fmt = std::path::Path::new(dest)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if json {
        print_json(&serde_json::json!({ "from": origin, "to": dest, "format": fmt }));
    } else {
        println!("Convertido: {origin} → {dest}");
    }
}

fn cmd_create(path: Option<String>, doc: Option<String>, sheet: Option<String>, slides: Option<String>, from: Option<String>, json: bool) {
    let targets = [doc.is_some(), sheet.is_some(), slides.is_some()].into_iter().filter(|b| *b).count();
    if path.is_some() && targets > 0 {
        fail("usage", "usá o un path local o --doc/--sheet/--slides, no ambos");
    }
    if let Some(p) = path {
        let ir = match &from {
            Some(f) => parse_ir(f),
            None => fail("usage", "create local requiere --from <ir.json|->"),
        };
        oxt_backend::create::create_from_ir(&p, &ir)
            .map_err(BackendError::from)
            .unwrap_or_else(|e| map_backend(e));
        if json {
            print_json(&serde_json::json!({ "path": p }));
        } else {
            println!("Creado: {p}");
        }
        return;
    }
    #[cfg(feature = "google")]
    {
        use oxt_backend::google;
        let (title, kind) = if let Some(t) = doc {
            (t, GoogleKind::Doc)
        } else if let Some(t) = sheet {
            (t, GoogleKind::Sheet)
        } else if let Some(t) = slides {
            (t, GoogleKind::Slides)
        } else {
            fail("usage", "create necesita un path local o --doc/--sheet/--slides");
        };
        let id = match kind {
            GoogleKind::Doc => google::create_doc(&title).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
            GoogleKind::Sheet => google::create_sheet(&title).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
            GoogleKind::Slides => google::create_slides(&title).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
        };
        if let Some(f) = &from {
            let ir = parse_ir(f);
            write_google(&id, Some(kind), &ir).unwrap_or_else(|e| map_backend(e));
        }
        let url = google_url(kind, &id);
        if json {
            print_json(&serde_json::json!({ "id": id, "url": url }));
        } else {
            println!("Creado: {url}");
        }
    }
    #[cfg(not(feature = "google"))]
    {
        let _ = (doc, sheet, slides, from, targets);
        fail("api_error", "Google no habilitado — compile con --features google")
    }
}

fn cmd_media(origin: &str, output: &str, json: bool) {
    use oxt_backend::media;
    let resolved = resolve(origin);
    let mut items = Vec::new();
    #[allow(unused_mut)] // mut solo con feature google (skipped de imágenes rotas)
    let mut skipped = Vec::new();
    match &resolved {
        Origin::Local(_) | Origin::Stdin => {
            let (ir, _) = open_local(&resolved);
            let (items_, skipped_) = media::collect_from_ir(&ir);
            items = items_;
            skipped = skipped_;
        }
        Origin::Google { id, kind } => {
            #[cfg(feature = "google")]
            {
                use oxt_backend::google::{self, GoogleImage};
                let images: Vec<GoogleImage> = match kind {
                    Some(GoogleKind::Doc) => google::fetch_doc_images(id).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
                    Some(GoogleKind::Slides) => google::fetch_slides_images(id).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
                    Some(GoogleKind::Sheet) => Vec::new(),
                    None => {
                        let (mime, _name) = google::get_file_metadata(id).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e));
                        match GoogleKind::from_mime(&mime) {
                            Some(GoogleKind::Doc) => google::fetch_doc_images(id).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
                            Some(GoogleKind::Slides) => google::fetch_slides_images(id).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e)),
                            _ => Vec::new(),
                        }
                    }
                };
                for (i, img) in images.into_iter().enumerate() {
                    let ir_path = format!("/google[{i}]");
                    match img.data {
                        Some(bytes) => items.push(media::MediaItem {
                            filename: img.filename,
                            data: bytes,
                            ir_path,
                        }),
                        None => skipped.push(media::MediaSkipped {
                            ir_path,
                            reason: "URL no disponible".into(),
                        }),
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                let _ = (id, kind, items, skipped);
                fail("api_error", "Google no habilitado — compile con --features google")
            }
        }
    }
    let mut result = media::write_media(items, std::path::Path::new(output)).unwrap_or_else(|e| {
        fail("io_error", format!("no se pudo escribir en {output}: {e}"))
    });
    result.skipped.extend(skipped);
    if json {
        print_json(&serde_json::to_value(&result).unwrap());
    } else {
        println!("Extraídos: {} archivos a {output}", result.files.len());
        for s in &result.skipped {
            eprintln!("⚠️  {}: {}", s.ir_path, s.reason);
        }
    }
}

// ── Google helpers ────────────────────────────────────────────────────────────

#[cfg(feature = "google")]
fn write_google(id: &str, kind: Option<GoogleKind>, ir: &OxtIR) -> oxt_backend::Result<()> {
    use oxt_backend::google;
    let kind = match kind {
        Some(k) => k,
        None => {
            let (mime, _name) = google::get_file_metadata(id)?;
            GoogleKind::from_mime(&mime)
                .ok_or_else(|| BackendError::Other(format!("no es un documento de Google soportado: {mime}")))?
        }
    };
    match kind {
        GoogleKind::Doc => google::write_doc(id, ir).map_err(BackendError::from),
        GoogleKind::Sheet => google::write_sheet(id, ir).map_err(BackendError::from),
        GoogleKind::Slides => google::write_slides(id, ir).map_err(BackendError::from),
    }
}

#[cfg(feature = "google")]
fn google_url(kind: GoogleKind, id: &str) -> String {
    let base = match kind {
        GoogleKind::Doc => "https://docs.google.com/document/d/",
        GoogleKind::Sheet => "https://docs.google.com/spreadsheets/d/",
        GoogleKind::Slides => "https://docs.google.com/presentation/d/",
    };
    format!("{base}{id}")
}

// ── schema ────────────────────────────────────────────────────────────────────

fn walk_commands(cmd: &clap::Command, prefix: &str, out: &mut Vec<serde_json::Value>) {
    for sub in cmd.get_subcommands() {
        if sub.get_name() == "help" {
            continue;
        }
        let name = if prefix.is_empty() {
            sub.get_name().to_string()
        } else {
            format!("{prefix} {}", sub.get_name())
        };
        let mut args = Vec::new();
        for a in sub.get_arguments() {
            if a.get_id() == "json" {
                continue; // global
            }
            args.push(serde_json::json!({
                "name": a.get_id().as_str(),
                "required": a.is_required_set(),
                "takes_value": a.get_action().takes_values(),
                "default": a.get_default_values().first().map(|v| v.to_string_lossy().to_string()),
                "choices": a.get_possible_values().iter().map(|v| v.get_name().to_string()).collect::<Vec<_>>(),
            }));
        }
        let mutates = matches!(
            sub.get_name(),
            "edit" | "update" | "create" | "convert" | "media" | "download" | "login" | "logout"
        );
        out.push(serde_json::json!({
            "name": name,
            "description": sub.get_about().map(|a| a.to_string()),
            "arguments": args,
            "mutates": mutates,
        }));
        walk_commands(sub, &name, out);
    }
}

fn cmd_schema() {
    let cmd = Cli::command();
    let mut commands = Vec::new();
    walk_commands(&cmd, "", &mut commands);
    print_json(&serde_json::json!({
        "schema_version": 1,
        "tool": "oxt",
        "version": env!("CARGO_PKG_VERSION"),
        "output": { "tty": "text", "piped": "text" },
        "global_args": [ { "name": "--json", "description": "Salida estructurada JSON" } ],
        "commands": commands,
        "errors": [
            { "kind": "usage", "exit": 2 },
            { "kind": "io_error", "exit": 3 },
            { "kind": "unsupported_format", "exit": 4 },
            { "kind": "parse_error", "exit": 4 },
            { "kind": "invalid_ir", "exit": 5 },
            { "kind": "auth_error", "exit": 6 },
            { "kind": "api_error", "exit": 7 },
            { "kind": "edit_error", "exit": 8 },
            { "kind": "internal_error", "exit": 10 }
        ],
        "outcomes": [
            { "code": 1, "meaning": "grep: sin matches; diff: hay diferencias; auth status: no autenticado (sin envelope de error)" }
        ],
        "formats": ["text", "markdown", "ir", "offset-map"],
        "extensions": ["docx", "xlsx", "pptx", "odt", "ods", "odp", "doc", "xls", "ppt"]
    }));
}

// ── Auth ──────────────────────────────────────────────────────────────────────

fn cmd_auth(sub: AuthCommand, json: bool) {
    match sub {
        AuthCommand::Login { client_id, client_secret, token } => {
            #[cfg(feature = "google")]
            {
                use oxt_backend::google;
                if let Some(t) = token {
                    google::save_refresh_token(&t).map_err(BackendError::from).unwrap_or_else(|e| map_backend(e));
                } else {
                    let result = match (client_id, client_secret) {
                        (Some(cid), Some(cs)) => google::authenticate(&cid, &cs),
                        _ => google::authenticate_defaults(),
                    };
                    result.map_err(BackendError::from).unwrap_or_else(|e| map_backend(e));
                }
                if json {
                    print_json(&serde_json::json!({ "status": "ok" }));
                } else {
                    println!("Autenticado");
                }
            }
            #[cfg(not(feature = "google"))]
            {
                let _ = (client_id, client_secret, token, json);
                fail("api_error", "Google no habilitado — compile con --features google")
            }
        }
        AuthCommand::Logout => {
            #[cfg(feature = "google")]
            {
                oxt_backend::google::logout().map_err(BackendError::from).unwrap_or_else(|e| map_backend(e));
                if json {
                    print_json(&serde_json::json!({ "status": "ok" }));
                } else {
                    println!("Sesión cerrada");
                }
            }
            #[cfg(not(feature = "google"))]
            {
                let _ = json;
                fail("api_error", "Google no habilitado — compile con --features google")
            }
        }
        AuthCommand::Status => {
            #[cfg(feature = "google")]
            {
                let source = oxt_backend::google::auth_source();
                let authenticated = source != "none";
                if json {
                    print_json(&serde_json::json!({ "authenticated": authenticated, "source": source }));
                } else {
                    println!("Autenticado: {}", if authenticated { "sí" } else { "no" });
                    println!("Fuente: {source}");
                }
                if !authenticated {
                    std::process::exit(1); // outcome
                }
            }
            #[cfg(not(feature = "google"))]
            {
                let _ = json;
                fail("api_error", "Google no habilitado — compile con --features google")
            }
        }
    }
}

fn cmd_list(query: Option<String>, json: bool) {
    #[cfg(feature = "google")]
    {
        let files = oxt_backend::google::list_drive_files(query.as_deref())
            .map_err(BackendError::from)
            .unwrap_or_else(|e| map_backend(e));
        if json {
            print_json(&files);
        } else if let Some(arr) = files.as_array() {
            for f in arr {
                let id = f.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("");
                println!("{id}\t{name}");
            }
        }
    }
    #[cfg(not(feature = "google"))]
    {
        let _ = (query, json);
        fail("api_error", "Google no habilitado — compile con --features google")
    }
}

fn cmd_download(id_or_url: &str, output: &str, json: bool) {
    let id = match resolve(id_or_url) {
        Origin::Google { id, .. } => id,
        _ => fail("usage", "download espera un ID o URL de Google Drive"),
    };
    #[cfg(feature = "google")]
    {
        oxt_backend::google::download_drive_file(&id, output)
            .map_err(BackendError::from)
            .unwrap_or_else(|e| map_backend(e));
        if json {
            print_json(&serde_json::json!({ "path": output }));
        } else {
            println!("Descargado: {output}");
        }
    }
    #[cfg(not(feature = "google"))]
    {
        let _ = (id, output, json);
        fail("api_error", "Google no habilitado — compile con --features google")
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // help/version no son errores: salida normal a stdout, exit 0
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                e.exit();
            }
            fail_hint("usage", e, None)
        }
    };
    let json = cli.json;

    match cli.command {
        Command::Read { origin, format } => cmd_read(&origin, &format, json),
        Command::Info { origin } => cmd_info(&origin, json),
        Command::Stats { origin, per_section } => cmd_stats(&origin, per_section, json),
        Command::Grep { pattern, origin, literal, ignore_case } => {
            cmd_grep(&pattern, &origin, literal, ignore_case, json)
        }
        Command::Diff { a, b } => cmd_diff(&a, &b, json),
        Command::Edit { origin, old, new } => cmd_edit(&origin, &old, &new, json),
        Command::Update { origin, from } => cmd_update(&origin, &from, json),
        Command::Convert { origin, dest } => cmd_convert(&origin, &dest, json),
        Command::Create { path, doc, sheet, slides, from } => {
            cmd_create(path, doc, sheet, slides, from, json)
        }
        Command::Media { origin, output } => cmd_media(&origin, &output, json),
        Command::List { query } => cmd_list(query, json),
        Command::Download { id, output } => cmd_download(&id, &output, json),
        Command::Schema => cmd_schema(),
        Command::Auth { command } => cmd_auth(command, json),
    }
}
