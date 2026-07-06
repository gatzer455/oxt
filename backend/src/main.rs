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
enum Command {
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

fn main() {
    let cli = Cli::parse();

    match cli.command {
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
        Command::Edit { path, old, new, json } => {
            match oxt_backend::edit::replace_text(&path, &old, &new) {
                Ok(result) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&result).unwrap());
                    } else {
                        println!("Reemplazos: {}", result.replacements);
                        if !result.affected_parts.is_empty() {
                            println!("Partes afectadas: {}", result.affected_parts.join(", "));
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
