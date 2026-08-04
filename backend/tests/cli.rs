//! Tests de integración de la CLI — humo por verbo contra el binario real.
//! Verifica exit codes y formas JSON del contrato (`docs/cli.md`).
//! Sin red, sin Google.

use std::path::PathBuf;
use std::process::{Command, Output};

const IR: &str = r#"{"metadata":{"title":"t"},"sections":[{"title":"Intro","elements":[
  {"kind":"heading","level":1,"text":"Hola mundo"},
  {"kind":"paragraph","runs":[{"text":"texto de prueba","bold":true}]},
  {"kind":"list","ordered":false,"items":["uno","dos"]},
  {"kind":"table","rows":[["a","b"],["c","d"]]}
]}]}"#;

struct Ctx {
    dir: PathBuf,
}

impl Ctx {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("oxt_cli_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Ctx { dir }
    }

    fn file(&self, name: &str, content: &str) -> PathBuf {
        let p = self.dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    fn docx(&self, name: &str) -> PathBuf {
        let p = self.file(name, IR);
        let out = self.dir.join(name);
        let r = run(&["create", out.to_str().unwrap(), "--from", p.to_str().unwrap()]);
        assert!(r.status.success(), "create falló: {}", String::from_utf8_lossy(&r.stderr));
        out
    }

    fn run(&self, args: &[&str]) -> Output {
        run(args)
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxt"))
        .args(args)
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap()
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!("stdout no es JSON: {e}\n{}", String::from_utf8_lossy(&out.stdout))
    })
}

fn err_json(out: &Output) -> serde_json::Value {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let last = stderr.lines().last().unwrap_or("");
    serde_json::from_str(last).unwrap_or_else(|e| {
        panic!("stderr no termina en JSON: {e}\n{stderr}")
    })
}

#[test]
fn create_and_read_roundtrip() {
    let c = Ctx::new("read");
    let doc = c.docx("d.docx");

    // read --json markdown
    let out = c.run(&["read", doc.to_str().unwrap(), "--json"]);
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["format"], "markdown");
    assert!(v["data"].as_str().unwrap().contains("Hola mundo"));

    // read --format ir
    let out = c.run(&["read", doc.to_str().unwrap(), "--format", "ir", "--json"]);
    let v = json(&out);
    assert_eq!(v["format"], "ir");
    let elems = v["data"]["sections"][0]["elements"].as_array().unwrap();
    let text = serde_json::to_string(elems).unwrap();
    assert!(text.contains("Hola mundo"), "el IR debe contener el texto: {text}");
}

#[test]
fn read_from_stdin() {
    let c = Ctx::new("stdin");
    let doc = c.docx("d.docx");
    let bytes = std::fs::read(&doc).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oxt"));
    cmd.args(["read", "-", "--format", "text", "--json"])
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), &bytes).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "stdin read falló: {}", String::from_utf8_lossy(&out.stderr));
    let v = json(&out);
    assert!(v["data"].as_str().unwrap().contains("Hola mundo"));
}

#[test]
fn info_and_stats_json() {
    let c = Ctx::new("info");
    let doc = c.docx("d.docx");

    let out = c.run(&["info", doc.to_str().unwrap(), "--json"]);
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["format"], "docx");
    assert!(v["sections"].as_u64().unwrap() >= 1);

    let out = c.run(&["stats", doc.to_str().unwrap(), "--json"]);
    assert!(out.status.success());
    let v = json(&out);
    assert!(v["words"].as_u64().unwrap() > 0);
    assert!(v["tables"].as_u64().unwrap() >= 1);
}

#[test]
fn grep_matches_and_outcome() {
    let c = Ctx::new("grep");
    let doc = c.docx("d.docx");

    let out = c.run(&["grep", "prueba", doc.to_str().unwrap(), "--json"]);
    assert!(out.status.success(), "grep con match debe salir 0");
    let v = json(&out);
    assert!(!v["matches"].as_array().unwrap().is_empty());
    let m = &v["matches"][0];
    assert!(m["path"].as_str().unwrap().contains("/s[0]/"));
    assert!(m["offset"].is_number());

    // outcome: sin matches → exit 1, sin envelope, JSON igual
    let out = c.run(&["grep", "zzzznope", doc.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(1), "sin matches debe salir 1");
    let v = json(&out);
    assert!(v["matches"].as_array().unwrap().is_empty());

    // --literal trata la regex como texto
    let out = c.run(&["grep", "(", doc.to_str().unwrap(), "--literal", "--json"]);
    assert!(out.status.success() || out.status.code() == Some(1));
}

#[test]
fn edit_and_update() {
    let c = Ctx::new("edit");
    let doc = c.docx("d.docx");

    let out = c.run(&["edit", doc.to_str().unwrap(), "--old", "prueba", "--new", "ensayo", "--json"]);
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["replacements"], 1);
    assert_eq!(v["changed"], true);

    // edit sin matches → exit 0, changed false
    let out = c.run(&["edit", doc.to_str().unwrap(), "--old", "noexiste", "--new", "x", "--json"]);
    assert!(out.status.success());
    assert_eq!(json(&out)["changed"], false);

    // update desde IR
    let ir2 = c.file("ir2.json", r#"{"sections":[{"title":"Nueva","elements":[{"kind":"paragraph","runs":[{"text":"contenido reemplazado"}]}]}]}"#);
    let out = c.run(&["update", doc.to_str().unwrap(), "--from", ir2.to_str().unwrap(), "--json"]);
    assert!(out.status.success(), "update falló: {}", String::from_utf8_lossy(&out.stderr));
    let out = c.run(&["read", doc.to_str().unwrap(), "--format", "text"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("contenido reemplazado"));
}

#[test]
fn update_from_stdin() {
    let c = Ctx::new("upstdin");
    let doc = c.docx("d.docx");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oxt"));
    cmd.args(["update", doc.to_str().unwrap(), "--from", "-", "--json"])
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), IR.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "update stdin falló: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn convert_local() {
    let c = Ctx::new("convert");
    let doc = c.docx("d.docx");
    let odt = c.dir.join("d.odt");
    let out = c.run(&["convert", doc.to_str().unwrap(), odt.to_str().unwrap(), "--json"]);
    assert!(out.status.success(), "convert falló: {}", String::from_utf8_lossy(&out.stderr));
    let v = json(&out);
    assert_eq!(v["format"], "odt");
    assert!(odt.exists());
}

#[test]
fn diff_outcome() {
    let c = Ctx::new("diff");
    let a = c.docx("a.docx");
    let b = c.docx("b.docx");
    let ir2 = c.file("ir2.json", r#"{"sections":[{"title":"Otra","elements":[{"kind":"paragraph","runs":[{"text":"otro contenido"}]}]}]}"#);
    let _ = c.run(&["update", b.to_str().unwrap(), "--from", ir2.to_str().unwrap()]);

    let out = c.run(&["diff", a.to_str().unwrap(), b.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(1), "con diferencias debe salir 1");
    let v = json(&out);
    assert_eq!(v["equal"], false);
    assert!(!v["changes"].as_array().unwrap().is_empty());

    let out = c.run(&["diff", a.to_str().unwrap(), a.to_str().unwrap(), "--json"]);
    assert!(out.status.success());
    assert_eq!(json(&out)["equal"], true);
}

#[test]
fn media_extracts_png() {
    // docx con imagen embebida (drawing + media part)
    let c = Ctx::new("media");
    let doc = c.dir.join("img.docx");
    let png = [0x89u8, 0x50, 0x4E, 0x47, 0x00];
    {
        use std::io::Write;
        let f = std::fs::File::create(&doc).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("[Content_Types].xml", opts).unwrap();
        z.write_all(br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#).unwrap();
        z.start_file("_rels/.rels", opts).unwrap();
        z.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#).unwrap();
        z.start_file("word/_rels/document.xml.rels", opts).unwrap();
        z.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/foto1.png"/></Relationships>"#).unwrap();
        z.start_file("word/document.xml", opts).unwrap();
        z.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p><w:r><w:drawing><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId5"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></w:drawing></w:r></w:p></w:body></w:document>"#).unwrap();
        z.start_file("word/media/foto1.png", opts).unwrap();
        z.write_all(&png).unwrap();
        z.finish().unwrap();
    }
    let out_dir = c.dir.join("out");
    let out = c.run(&["media", doc.to_str().unwrap(), "--output", out_dir.to_str().unwrap(), "--json"]);
    assert!(out.status.success(), "media falló: {}", String::from_utf8_lossy(&out.stderr));
    let v = json(&out);
    let files = v["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "debe extraer 1 imagen: {v}");
    assert_eq!(files[0]["filename"], "foto1.png");
    assert!(out_dir.join("foto1.png").exists());
}

#[test]
fn schema_is_valid_json() {
    let out = run(&["schema"]);
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["tool"], "oxt");
    let names: Vec<&str> = v["commands"].as_array().unwrap()
        .iter().map(|c| c["name"].as_str().unwrap()).collect();
    for expected in ["read", "edit", "update", "create", "grep", "diff", "stats", "media", "schema", "auth"] {
        assert!(names.contains(&expected), "falta {expected} en schema: {names:?}");
    }
}

#[test]
fn error_envelopes() {
    let c = Ctx::new("errs");
    // origen no reconocido → usage / 2
    let out = c.run(&["read", "no_existe_asi.docx", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let v = err_json(&out);
    assert_eq!(v["kind"], "usage");

    // archivo existente pero no soportado → unsupported_format / 4
    let txt = c.file("nota.txt", "hola");
    let out = c.run(&["read", txt.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(err_json(&out)["kind"], "unsupported_format");

    // IR inválido → invalid_ir / 5
    let bad = c.file("bad.json", "{no es json");
    let out = c.run(&["create", c.dir.join("x.docx").to_str().unwrap(), "--from", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(5));
    assert_eq!(err_json(&out)["kind"], "invalid_ir");

    // clap: falta el origen → usage / 2
    let out = c.run(&["read"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(err_json(&out)["kind"], "usage");

    // formato inválido → usage / 2
    let doc = c.docx("d.docx");
    let out = c.run(&["read", doc.to_str().unwrap(), "--format", "xml"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(err_json(&out)["kind"], "usage");
}

#[test]
fn help_shows_flat_verbs() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    for verb in ["read", "edit", "update", "create", "convert", "grep", "diff", "stats", "media", "schema", "auth"] {
        assert!(help.contains(verb), "falta {verb} en help");
    }
    assert!(!help.contains("docs:read"), "no debe quedar el namespace google");
}
