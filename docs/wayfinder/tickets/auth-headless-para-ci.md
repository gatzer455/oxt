---
titulo: Auth headless para CI
tipo: wayfinder:interrogatorio
estado: abierto
asignado: pi (sesión 2026-07-30)
estado: cerrado
bloqueado-por: []
---

## Pregunta

¿Cómo autentica oxt sin navegador interactivo —CI, servidores, contenedores— sin que un humano haga clic?

Graduado de la niebla al resolver «Forma plana de la CLI»: `auth login|logout|status` ya está decidido; falta el mecanismo no interactivo. Un harness en CI no puede abrir un navegador ni recibir el redirect loopback.

Ramas:

- Mecanismo: ¿device flow de Google OAuth (¿sigue soportado? — verificar), inyección de token por env var (`OXT_GOOGLE_TOKEN`), o flag `auth login --token <refresh|access>`?
- Guardado: si el token entra por env var, ¿se guarda igual en `~/.config/oxt/google-tokens.json` o se usa solo en memoria para la invocación?
- Interacción con `auth status`: ¿reporta distinto según el origen del token?
- Seguridad: token por env var en CI — ¿documentar redirección a secret store del CI?

Criterio de salida: un harness en CI puede autenticarse sin interacción humana y operar, con el mecanismo documentado en el contrato.

## Resolución (2026-07-30, investigación + decisión en ráfaga — provisional)

**El device flow de Google NO sirve**: la doc oficial (OAuth 2.0 for TV and Limited-Input Devices, verificada 2026-07) restringe el flujo a scopes de YouTube — Docs/Sheets/Slides/Drive quedan fuera.

Mecanismo adoptado (documentado en `docs/cli.md` §Auth): **inyección de refresh token**.

- `OXT_GOOGLE_TOKEN=<refresh_token>` en env → token en memoria para la invocación, sin escribir el archivo de config.
- `oxt auth login --token <refresh>` → persiste el token out-of-band en `~/.config/oxt/google-tokens.json`.
- Flujo CI documentado: generar token una vez con `oxt auth login` en desktop → copiarlo al secret store del CI → `OXT_GOOGLE_TOKEN` por invocación.
- `auth status` reporta `source: file|env|none`.

El refresh automático existente (access token expirado → refresh) aplica igual para tokens de env.
