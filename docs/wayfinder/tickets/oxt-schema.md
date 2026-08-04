---
titulo: oxt schema
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: [forma-plana-de-la-cli]
---

## Pregunta

¿Qué contiene exactamente el JSON de `oxt schema` —estructura, campos, formatos válidos, exit codes— y cómo se genera sin duplicar a mano el derive de clap?

Decidido al trazar: subcomando `oxt schema` que emite el CLI en JSON, para que un harness descubra la CLI sin leer `--help`.

Ramas:

- Contenido: por cada verbo — nombre, descripción, posicionales (nombre, requerido, default), flags (nombre largo/corto, tipo, default, valores válidos), y las tablas transversales (formatos de salida válidos, extensiones soportadas, exit codes).
- Generación: ¿derivar de la definición clap en runtime (crate `clap` expone `Command::get_arguments()` — suficiente?), ¿`clap_complete` + parseo, o definición manual duplicada? Costo y riesgo de cada una.
- Forma del JSON de schema: envoltura vs plano; versionado (¿campo `schema_version` para que los harnesses migren?).
- Relación con la especificación escrita del contrato (`docs/cli.md`): ¿el schema se genera desde la misma fuente de verdad que el doc, o el doc referencia el schema?

Criterio de salida: el JSON de `oxt schema` está especificado campo por campo y el mecanismo de generación elegido.

## Resolución (2026-07-30, ráfaga autorizada — provisional)

Especificado en `docs/cli.md` §oxt schema:

- Contenido: `{schema_version: 1, tool, version, output: {tty: text, piped: text}, global_args, commands: [{name, description, arguments: [{name, required, takes_value, default, choices}], mutates}], errors: [{kind, exit}], outcomes, formats, extensions}`.
- Generación: **runtime walk del árbol de clap** (`get_subcommands`, `get_arguments`, `get_default_values`, `get_possible_values`) — sin duplicar definiciones; las tablas transversales (errors/outcomes/formats/extensions) viven en una constante Rust en main.rs.
- `schema` emite JSON siempre, sin `--json`.
- Los `kind` de error declarados en el schema son la fuente de verdad que los harnesses usan para branch sin parsear mensajes.
