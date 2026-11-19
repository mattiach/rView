# UI languages

Every `<code>.ftl` file in this folder is a UI language, embedded in rView when it is built
(`app/build.rs` picks up whatever is here: no code to change). `en.ftl` is the source and the
fallback: a message missing in a translation is shown in English.

## Adding a language

1. Copy `en.ftl` to `<code>.ftl`, where `<code>` is a language tag: `it`, `de`, `pt-BR`, `zh-Hans`…
2. Translate the text after each `=`; never change the ids before it. Keep `{ $name }` arguments as
   they are (their position in the sentence can change). Set `language-name` to the language's own
   name ("Italiano"): Settings → Miscellaneous → Language shows it.
3. In menu texts `&` marks the access-key letter (`&File`), `&&` is a literal `&`.
4. Build. The language appears in Settings; with "System default" it is chosen when Windows uses it
   (`it-IT` → `it`; no match → English).

Unfinished files are fine: missing messages fall back to English, and an entry with a syntax error
is skipped (logged) without breaking the others.

## Checks

`cargo test -p rview --lib i18n` checks that `en.ftl` parses and contains every id the code uses.
Syntax: <https://projectfluent.org/fluent/guide/>.
