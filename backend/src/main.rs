#![allow(unused_variables)]
//! oxt — CLI para documentos de oficina
//! Lee, edita y crea documentos DOCX/XLSX/PPTX/ODF.
//! Diseñado para ser usado por agentes LLM (pi).

use clap::{Parser, Subcommand};
use oxt_backend::agent::ReadOutput;
use oxt_backend::Document;

#[derive(Parser)]
#[command(name = "oxt", about = "Documentos de oficina para LLMs", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum GoogleCommand {
    /// Autenticar con Google Workspace
    Auth {
        /// Client ID de GCP (opcional, default: credenciales embebidas)
        #[arg(long)]
        client_id: Option<String>,
        /// Client Secret de GCP (opcional, default: credenciales embebidas)
        #[arg(long)]
        client_secret: Option<String>,
    },
    /// Leer un Google Doc
    #[command(name = "docs:read")]
    DocsRead {
        /// ID del documento
        document_id: String,
        /// Formato de salida: text, markdown, ir
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Crear un Google Doc
    #[command(name = "docs:create")]
    DocsCreate {
        /// Título del documento
        title: String,
    },
    /// Actualizar un Google Doc desde un archivo IR
    #[command(name = "docs:update")]
    DocsUpdate {
        /// ID del documento
        document_id: String,
        /// Archivo JSON con el OxtIR
        #[arg(long)]
        from: String,
    },

    /// Leer un Google Sheet
    #[command(name = "sheets:read")]
    SheetsRead {
        /// ID del spreadsheet
        spreadsheet_id: String,
        /// Formato de salida: text, markdown, ir
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Crear un Google Sheet
    #[command(name = "sheets:create")]
    SheetsCreate {
        /// Título del spreadsheet
        title: String,
    },
    /// Actualizar un Google Sheet desde un archivo IR
    #[command(name = "sheets:update")]
    SheetsUpdate {
        /// ID del spreadsheet
        spreadsheet_id: String,
        /// Archivo JSON con el OxtIR
        #[arg(long)]
        from: String,
    },

    /// Leer una presentación de Google Slides
    #[command(name = "slides:read")]
    SlidesRead {
        /// ID de la presentación
        presentation_id: String,
        /// Formato de salida: text, markdown, ir
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Crear una presentación en Google Slides
    #[command(name = "slides:create")]
    SlidesCreate {
        /// Título de la presentación
        title: String,
    },
    /// Actualizar una presentación desde un archivo IR
    #[command(name = "slides:update")]
    SlidesUpdate {
        /// ID de la presentación
        presentation_id: String,
        /// Archivo JSON con el OxtIR
        #[arg(long)]
        from: String,
    },
}

#[derive(Subcommand)]
enum Command {
    /// Comandos de Google Workspace
    #[command(name = "google")]
    Google {
        #[command(subcommand)]
        command: GoogleCommand,
    },
    /// Leer un documento y mostrarlo en el formato indicado.
    Read {
        /// Ruta al archivo
        path: String,

        /// Formato de salida: text, markdown, ir, offset-map
        #[arg(long, default_value = "markdown")]
        format: String,

        /// Salida como JSON (envoltura)
        #[arg(long)]
        json: bool,
    },

    /// Mostrar información del documento.
    Info {
        /// Ruta al archivo
        path: String,

        /// Salida como JSON
        #[arg(long)]
        json: bool,
    },

    /// Crear un documento desde un archivo JSON (IR).
    Create {
        /// Ruta de salida (ej: reporte.docx)
        path: String,

        /// Archivo JSON con el IR
        #[arg(long)]
        from: String,
    },

    /// Reemplazar texto en un documento.
    Edit {
        /// Ruta al archivo
        path: String,

        /// Texto a reemplazar
        #[arg(long)]
        old: String,

        /// Texto nuevo
        #[arg(long)]
        new: String,

        /// Salida como JSON
        #[arg(long)]
        json: bool,
    },
}

fn handle_google(cmd: GoogleCommand) {
    match cmd {
        GoogleCommand::Auth { client_id, client_secret } => {
            let result = match (client_id, client_secret) {
                (Some(cid), Some(cs)) => oxt_backend::google::authenticate(&cid, &cs),
                _ => oxt_backend::google::authenticate_defaults(),
            };
            match result {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        GoogleCommand::DocsRead { document_id, format } => {
            #[cfg(feature = "google")]
            {
                match oxt_backend::google::read_doc(&document_id) {
                    Ok(ir) => {
                        let output = match format.as_str() {
                            "ir" => serde_json::to_string_pretty(&ir).unwrap_or_default(),
                            "markdown" => ir.to_markdown(),
                            _ => ir.plain_text(),
                        };
                        println!("{output}");
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                eprintln!("Error: Google feature no habilitada. Compile con --features google");
                std::process::exit(1);
            }
        }
        GoogleCommand::DocsCreate { title } => {
            #[cfg(feature = "google")]
            {
                match oxt_backend::google::create_doc(&title) {
                    Ok(id) => println!("Creado: https://docs.google.com/document/d/{id}"),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                eprintln!("Error: Google feature no habilitada");
                std::process::exit(1);
            }
        }
        GoogleCommand::SheetsRead { spreadsheet_id, format } => {
            #[cfg(feature = "google")]
            {
                match oxt_backend::google::read_sheet(&spreadsheet_id) {
                    Ok(ir) => {
                        let output = match format.as_str() {
                            "ir" => serde_json::to_string_pretty(&ir).unwrap_or_default(),
                            "markdown" => ir.to_markdown(),
                            _ => ir.plain_text(),
                        };
                        println!("{output}");
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                eprintln!("Error: Google feature no habilitada");
                std::process::exit(1);
            }
        }
        GoogleCommand::SheetsCreate { title } => {
            #[cfg(feature = "google")]
            {
                match oxt_backend::google::create_sheet(&title) {
                    Ok(id) => println!("Creado: https://docs.google.com/spreadsheets/d/{id}"),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                eprintln!("Error: Google feature no habilitada");
                std::process::exit(1);
            }
        }
        GoogleCommand::SlidesRead { presentation_id, format } => {
            #[cfg(feature = "google")]
            {
                match oxt_backend::google::read_slides(&presentation_id) {
                    Ok(ir) => {
                        let output = match format.as_str() {
                            "ir" => serde_json::to_string_pretty(&ir).unwrap_or_default(),
                            "markdown" => ir.to_markdown(),
                            _ => ir.plain_text(),
                        };
                        println!("{output}");
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                eprintln!("Error: Google feature no habilitada");
                std::process::exit(1);
            }
        }
        GoogleCommand::SlidesCreate { title } => {
            #[cfg(feature = "google")]
            {
                match oxt_backend::google::create_slides(&title) {
                    Ok(id) => println!("Creado: https://docs.google.com/presentation/d/{id}"),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                eprintln!("Error: Google feature no habilitada");
                std::process::exit(1);
            }
        }
        GoogleCommand::SlidesUpdate { presentation_id, from } => {
            #[cfg(feature = "google")]
            {
                let json_data = match std::fs::read_to_string(&from) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("Error leyendo {from}: {e}");
                        std::process::exit(1);
                    }
                };
                let ir: oxt_backend::ir::OxtIR = match serde_json::from_str(&json_data) {
                    Ok(ir) => ir,
                    Err(e) => {
                        eprintln!("Error parseando IR: {e}");
                        std::process::exit(1);
                    }
                };
                match oxt_backend::google::write_slides(&presentation_id, &ir) {
                    Ok(_) => println!("Presentación actualizada: {presentation_id}"),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                eprintln!("Error: Google feature no habilitada");
                std::process::exit(1);
            }
        }
        GoogleCommand::SheetsUpdate { spreadsheet_id, from } => {
            #[cfg(feature = "google")]
            {
                let json_data = match std::fs::read_to_string(&from) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("Error leyendo {from}: {e}");
                        std::process::exit(1);
                    }
                };
                let ir: oxt_backend::ir::OxtIR = match serde_json::from_str(&json_data) {
                    Ok(ir) => ir,
                    Err(e) => {
                        eprintln!("Error parseando IR: {e}");
                        std::process::exit(1);
                    }
                };
                match oxt_backend::google::write_sheet(&spreadsheet_id, &ir) {
                    Ok(_) => println!("Sheet actualizado: {spreadsheet_id}"),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                eprintln!("Error: Google feature no habilitada");
                std::process::exit(1);
            }
        }
        GoogleCommand::DocsUpdate { document_id, from } => {
            #[cfg(feature = "google")]
            {
                let json_data = match std::fs::read_to_string(&from) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("Error leyendo {from}: {e}");
                        std::process::exit(1);
                    }
                };
                let ir: oxt_backend::ir::OxtIR = match serde_json::from_str(&json_data) {
                    Ok(ir) => ir,
                    Err(e) => {
                        eprintln!("Error parseando IR: {e}");
                        std::process::exit(1);
                    }
                };
                match oxt_backend::google::write_doc(&document_id, &ir) {
                    Ok(_) => println!("Documento actualizado: {document_id}"),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "google"))]
            {
                eprintln!("Error: Google feature no habilitada");
                std::process::exit(1);
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Google { command } => {
            handle_google(command);
        }

        Command::Read { path, format, json } => {
            match Document::open(&path) {
                Ok(doc) => {
                    let output = ReadOutput::from_ir(doc.to_ir(), &doc.format().to_string());

                    if json {
                        let value = serde_json::to_value(&output).unwrap_or_default();
                        println!("{}", serde_json::to_string_pretty(&value).unwrap());
                        return;
                    }

                    match format.as_str() {
                        "text" => println!("{}", output.plain_text),
                        "markdown" => println!("{}", output.markdown),
                        "ir" => println!("{}", serde_json::to_string_pretty(&output.ir_json).unwrap()),
                        "offset-map" | "offsets" => {
                            println!("{}", serde_json::to_string_pretty(&output.offset_map).unwrap());
                        }
                        _ => {
                            eprintln!("Formato no válido: {format}. Usa: text, markdown, ir, offset-map");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Create { path, from } => {
            match oxt_backend::create::create_from_json(&path, &from) {
                Ok(()) => println!("Creado: {path}"),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }

        Command::Edit { path, old, new, json } => {
            match oxt_backend::edit::replace_text(&path, &old, &new) {
                Ok(result) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&result).unwrap());
                    } else {
                        println!("Reemplazos: {}", result.replacements);
                        if !result.affected_parts.is_empty() {
                            println!("Partes afectadas: {}", result.affected_parts.join(", "));
                            // Mostrar advertencia si hubo conversión de formato
                            for part in &result.affected_parts {
                                if part.starts_with("convertido") {
                                    eprintln!("⚠️  {}", part);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Info { path, json } => {
            match Document::open(&path) {
                Ok(doc) => {
                    let ir = doc.to_ir();
                    let element_count: usize = ir.sections.iter()
                        .map(|s| s.elements.len())
                        .sum();

                    if json {
                        let info = serde_json::json!({
                            "path": doc.path(),
                            "format": doc.format().to_string(),
                            "sections": ir.sections.len(),
                            "elements": element_count,
                            "title": ir.metadata.title,
                        });
                        println!("{}", serde_json::to_string_pretty(&info).unwrap());
                    } else {
                        println!("Archivo: {}", doc.path());
                        println!("Formato: {}", doc.format());
                        println!("Secciones: {}", ir.sections.len());
                        println!("Elementos: {element_count}");
                        if let Some(ref title) = ir.metadata.title {
                            println!("Título: {title}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
