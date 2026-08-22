# Bundled fonts

Alfred bundles variable WOFF2 releases from each typeface's upstream project so
the desktop UI does not require a network request at runtime.

| Family | File | Source |
| --- | --- | --- |
| Geist variable | `geist-variable.woff2` | [Vercel Geist](https://github.com/vercel/geist-font) |
| Geist Mono variable | `geist-mono-variable.woff2` | [Vercel Geist](https://github.com/vercel/geist-font) |
| Fraunces variable | `fraunces-variable.woff2` | [Fraunces](https://github.com/undercasetype/Fraunces) |

Geist and Geist Mono are active in the release UI. Fraunces is retained as a
licensed legacy asset but is no longer loaded. The primary CSS stack prefers
Infer and falls back to Geist until a licensed Infer WOFF2 file is supplied.

The bundled families are distributed under the SIL Open Font License 1.1. The
license text for each project is stored beside the font files. If smaller or
broader character-set support is required, replace the local assets with the
appropriate subsets instead of adding a remote CSS import.
