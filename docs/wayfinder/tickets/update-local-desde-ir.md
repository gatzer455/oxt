---
titulo: update local desde IR
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: [forma-plana-de-la-cli, contrato-de-salida-y-errores-json]
---

## Pregunta

¿Cuál es el contrato del verbo `update` para archivos locales —argumentos, semántica de preservación, salida JSON— que reemplaza el contenido de un documento existente con un OxtIR?

Hoy un harness solo puede `edit` texto puntual o `create` un archivo nuevo. Para Google ya existe el análogo (`docs:update`/`sheets:update`/`slides:update` con `--from`).

Hechos del motor: `roundtrip.rs` ya regenera los XMLs de las partes principales desde un OxtIR (`get_regenerated_files()`) y reempaqueta el ZIP preservando el resto; `edit.rs` usa `replace_in_ir` sobre el IR antes de regenerar. Un `update` es: abrir → reemplazar el IR completo → regenerar → guardar.

Ramas:

- Argumentos: ¿`oxt update <origen> --from ir.json|-` (mismo contrato que `create`)?
- Semántica: ¿reemplaza TODO el contenido (secciones/elementos) o solo lo que el IR nuevo menciona? ¿Qué pasa con metadata no cubierta por el IR?
- Legacy (.doc/.xls/.ppt): ¿convierte a OOXML como `edit` (cambia extensión) o rechaza?
- Salida JSON: ¿campos? (¿ruta, reemplazos?, ¿advertencias de conversión?)
- Relación con `create`: ¿`update` = `create` sobre archivo existente, o verbos distintos?

Criterio de salida: un harness sabe invocar `update` y qué esperar en éxito y error, sin ambigüedad.

## Resolución (2026-07-30, ráfaga autorizada — provisional)

Contrato en `docs/cli.md` (árbol + §Salida JSON por comando):

- `oxt update <origen> --from ir.json|- [--json]` — mismo contrato de `--from` que `create`.
- Semántica: reemplaza TODO el contenido (secciones/elementos) del IR nuevo; las partes no principales del ZIP se preservan (mismo preservation bag de `edit`). Metadata del IR nuevo se aplica si viene; si no, se conserva la del documento.
- Local: nuevo método en `roundtrip.rs` (`replace_ir_and_save(path, &ir)`) — abrir → reemplazar IR → regenerar XMLs → reempaquetar. Google: `write_doc/write_sheet/write_slides` existentes.
- Legacy: convierte a OOXML como `edit` (extensión cambia); la salida lo reporta.
- Salida JSON: local `{"path", "format", "converted_from"?}`; Google `{"id", "url"}`. Sin campo `changed` (siempre muta).
