# Fonts

Lato, the default font of Markup text boxes, embedded in `rview.exe` so that projects
and exports look the same on every PC.

- Source: the official Google Fonts repository, `https://github.com/google/fonts/tree/main/ofl/lato`
  (downloaded 14 Nov 2026).
- Licence: SIL Open Font License 1.1, `OFL.txt` (Copyright (c) 2010-2014 by tyPoland Lukasz Dziedzic).

| File | SHA-256 |
| --- | --- |
| Lato-Regular.ttf | `d636e4683231f931eda222d588e944d082bfd3bdba02f928bee461c0f185b251` |
| Lato-Bold.ttf | `8a0aace75d33794eece4b28187bfc1df0bbd2888b5d8a56e01788c8d65d16be1` |
| Lato-Italic.ttf | `e399c44efe1387100531d26c7e4800c5d12251b890d6654a3098c7c679cb1786` |
| Lato-BoldItalic.ttf | `62c1b7f0d2e74b45960154c3520efc337b553db0961bfdc950d5618334596cc8` |
| OFL.txt | `74ba064d03f1f1c4a952da936c3eb71866c34404916734de3cae73b34357e59e` |

Another default font: replace these files (same names) or change `DEFAULT_FAMILY` and `LATO` in
`core/src/edit/text.rs`. The text golden test (`core/tests/redact.rs`, `golden_text_with_lato`) pins
the rendering of these exact files.
