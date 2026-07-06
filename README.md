# 🐂 oxt

**Herramienta CLI para que agentes LLM lean, editen y creen documentos de oficina.**

`oxt` transforma documentos **DOCX, XLSX, PPTX** (y pronto ODF) en un **IR unificado (JSON)** que los LLMs pueden entender y manipular. Un solo comando, sin dependencias externas, sin Office instalado.

```bash
# Leer un documento como markdown
oxt read reporte.docx

# Obtener el IR como JSON estructurado (para el LLM)
oxt read matriz.xlsx --format ir

# Obtener mapa de offsets para ediciones precisas
oxt read carta.docx --format offset-map

# Metadatos
oxt info presentacion.pptx --json
```

## Filosofía

- **Minimalista.** Un solo binario. Sin FFI, sin bindings, sin WASM, sin runtime.
- **Para agentes.** El IR es el contrato: JSON que el LLM recibe, entiende y puede modificar.
- **Extensible.** Cada formato es un módulo. Todos convergen al mismo IR.

## Estado

| Formato | Leer | Editar | Crear |
|---------|------|--------|-------|
| DOCX | ✅ | — | — |
| XLSX | ✅ | — | — |
| PPTX | — | — | — |
| .doc / .xls / .ppt | — | — | — |
| ODF (.odt/.ods/.odp) | — | — | — |

## Instalación

```bash
cargo install oxt-backend
```

O desde source:

```bash
git clone https://github.com/gatzer455/oxt
cd oxt
cargo build --release
./target/release/oxt --help
```

## Integración con pi (xi)

`oxt` se integra con [pi](https://github.com/earendil-works/pi-coding-agent) a través de una extensión en el proyecto [xi](https://github.com/gatzer455/xi):

```
xi/packages/xi-office/index.ts  →  spawn("oxt", [...args])
```

---

Licencia: MIT / Apache-2.0
