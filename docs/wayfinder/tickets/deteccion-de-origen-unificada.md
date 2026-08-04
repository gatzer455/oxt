---
titulo: Detección de origen unificada
tipo: wayfinder:prototipo
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: [forma-plana-de-la-cli]
---

## Pregunta

Dado un argumento de origen/destino en cualquier verbo de la CLI plana, ¿qué reglas determinan si es path local, URL de Google o ID desnudo de Google, y dónde vive esa lógica?

Bloqueado por «Forma plana de la CLI»: la superficie decide qué argumentos llevan origen (posicional de `read`, `--from`, etc.).

Ramas:

- Reglas concretas: ¿URL completa (`https://docs.google.com/...`) siempre aceptada? ¿ID desnudo (regex ~30 chars alfanuméricos) aceptado? ¿ambiguo con nombres de archivo — se necesita flag `--google` o el ID exige URL?
- Dónde vive: ¿`lib.rs` (`Document::open` extiende), un resolver nuevo en `main.rs`, o el módulo `google.rs`?
- Cómo se infiere doc/sheet/slides desde el origen (URL lo lleva; ID desnudo no: ¿requiere URL, o intento+error?).

Prototipo: tabla de ejemplos de entrada → origen resuelto (path / google-doc / google-sheet / google-slides / error), para reaccionar ante ella. Enlazar el prototipo como artefacto en la resolución.

## Resolución (2026-07-30, ráfaga autorizada — provisional)

Reglas finales en `docs/cli.md` §Orígenes, módulo nuevo `origin.rs`:

1. `-` → stdin
2. existe como path local → local
3. URL de docs.google.com/drive.google.com con `/d/{id}` → Google con kind explícito de la URL (document/spreadsheets/presentation/file)
4. shape de ID `^[A-Za-z0-9_-]{25,}$` → Google con kind `None`, resuelto en runtime vía `files.get` de Drive API (una llamada, solo para IDs desnudos)
5. nada → error `usage` con hint

`Origin` = `Stdin | Local(PathBuf) | Google { id, kind: Option<DocKind> }`. La tabla de ejemplos (prototipo) queda como test cases en `origin.rs`: URL de doc/sheet/slides/file, ID desnudo, path existente, `-`, string raro → error.

Límite aceptado: un path local que además matchea el shape de ID se resuelve como local (regla 2 antes que 4).
