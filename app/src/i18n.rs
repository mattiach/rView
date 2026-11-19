//! UI strings. Every text the user reads goes through `t` / `tf` with a message id.
//! Languages are the Fluent files `assets/i18n/<code>.ftl` (embedded by build.rs: a new language
//! is a new file, see assets/i18n/README.md). English (`en.ftl`) is the source and the fallback:
//! a message missing in a translation is shown in English.
//! Menu texts keep the `&` mnemonic (muda uses it); egui texts have none.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use std::sync::{Arc, RwLock};
use unic_langid::LanguageIdentifier;

include!(concat!(env!("OUT_DIR"), "/languages.rs"));

pub const SOURCE: &str = "en";

struct Strings {
    code: String,
    /// The chosen language, then English.
    bundles: Vec<FluentBundle<FluentResource>>,
}

static STRINGS: RwLock<Option<Arc<Strings>>> = RwLock::new(None);

fn bundle(code: &str) -> Option<FluentBundle<FluentResource>> {
    let src = LANGUAGES.iter().find(|(c, _)| *c == code)?.1;
    Some(bundle_from(code, src))
}

/// A bundle from Fluent source; syntax errors skip only the broken entries.
fn bundle_from(code: &str, src: &str) -> FluentBundle<FluentResource> {
    let res = FluentResource::try_new(src.to_string()).unwrap_or_else(|(res, errors)| {
        log::warn!("i18n/{code}.ftl: {errors:?}");
        res
    });
    let lang: LanguageIdentifier = code.parse().unwrap_or_default();
    let mut b = FluentBundle::new_concurrent(vec![lang]);
    // no Unicode isolation marks around arguments: egui and Win32 menus would show them
    b.set_use_isolating(false);
    if let Err(e) = b.add_resource(res) {
        log::warn!("i18n/{code}.ftl: {e:?}");
    }
    b
}

/// The language's own name (message `language-name` of its file; the code if it has none).
pub fn language_name(code: &str) -> String {
    bundle(code)
        .and_then(|b| {
            let p = b.get_message("language-name")?.value()?;
            Some(b.format_pattern(p, None, &mut vec![]).into_owned())
        })
        .unwrap_or_else(|| code.to_string())
}

/// Codes of the embedded languages, English first.
pub fn available() -> Vec<&'static str> {
    let mut v: Vec<&str> = LANGUAGES.iter().map(|(c, _)| *c).collect();
    v.sort_by_key(|c| (*c != SOURCE, *c));
    v
}

/// Best available language for the user's preferences ("it-IT", "en-US", …): exact, then the
/// same language with another region, then English.
pub fn negotiate(requested: &[String], available: &[&str]) -> String {
    let req: Vec<LanguageIdentifier> = requested.iter().filter_map(|r| r.parse().ok()).collect();
    let avail: Vec<LanguageIdentifier> = available.iter().filter_map(|a| a.parse().ok()).collect();
    let default: LanguageIdentifier = SOURCE.parse().unwrap();
    fluent_langneg::negotiate_languages(
        &req,
        &avail,
        Some(&default),
        fluent_langneg::NegotiationStrategy::Filtering,
    )
    .first()
    .map(|l| l.to_string())
    .unwrap_or_else(|| SOURCE.into())
}

/// Use `setting` ("system" or a language code). Returns the code in use.
pub fn set_language(setting: &str) -> String {
    let requested = if setting.eq_ignore_ascii_case("system") {
        crate::platform::preferred_ui_languages()
    } else {
        vec![setting.to_string()]
    };
    let code = negotiate(&requested, &available());
    let mut bundles: Vec<_> = bundle(&code).into_iter().collect();
    if code != SOURCE {
        bundles.extend(bundle(SOURCE));
    }
    *STRINGS.write().unwrap() = Some(Arc::new(Strings {
        code: code.clone(),
        bundles,
    }));
    code
}

fn strings() -> Arc<Strings> {
    if let Some(s) = STRINGS.read().unwrap().as_ref() {
        return s.clone();
    }
    set_language(SOURCE);
    STRINGS.read().unwrap().clone().expect("strings set")
}

/// Language code in use.
pub fn current() -> String {
    strings().code.clone()
}

fn format(id: &str, args: Option<&FluentArgs>) -> String {
    format_in(&strings().bundles, id, args)
}

/// First bundle that has the message (the chosen language, then English).
fn format_in(bundles: &[FluentBundle<FluentResource>], id: &str, args: Option<&FluentArgs>) -> String {
    for b in bundles {
        if let Some(pattern) = b.get_message(id).and_then(|m| m.value()) {
            let mut errors = vec![];
            let text = b.format_pattern(pattern, args, &mut errors);
            if !errors.is_empty() {
                log::warn!("i18n {id}: {errors:?}");
            }
            return text.into_owned();
        }
    }
    log::warn!("i18n: no message {id}");
    id.to_string()
}

/// The text of message `id`.
pub fn t(id: &str) -> String {
    format(id, None)
}

/// The text of message `id` with named arguments (`{ $name }` in the .ftl file).
pub fn tf(id: &str, args: &[(&str, &str)]) -> String {
    let mut a = FluentArgs::new();
    for (k, v) in args {
        a.set(*k, FluentValue::from(*v));
    }
    format(id, Some(&a))
}

/// Without the `&` mnemonic markers (for egui; "&&" stays a literal "&").
pub fn plain(s: &str) -> String {
    s.replace("&&", "\u{0}").replace('&', "").replace('\u{0}', "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation() {
        let avail = ["en", "it", "pt-BR"];
        let n = |r: &[&str]| negotiate(&r.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &avail);
        assert_eq!(n(&["it-IT", "en-US"]), "it", "same language, other region");
        assert_eq!(n(&["pt-BR"]), "pt-BR");
        assert_eq!(n(&["pt-PT"]), "pt-BR", "closest region");
        assert_eq!(n(&["de-DE", "fr"]), "en", "nothing matches: English");
        assert_eq!(n(&[]), "en");
        assert_eq!(n(&["not a tag!!"]), "en");
        // with only English embedded, every preference ends in English
        assert_eq!(negotiate(&["it-IT".into()], &["en"]), "en");
    }

    /// What a translator's file does: its messages win, the missing ones fall back to English,
    /// arguments and multi-line messages work, broken entries don't break the rest.
    #[test]
    fn a_translation_with_fallback() {
        let it = "language-name = Italiano\naction-open = &Apri...\nrename-exists = Esiste già un file { $name }.\nbroken = { \n";
        let bundles = vec![bundle_from("it", it), bundle(SOURCE).unwrap()];
        let mut args = FluentArgs::new();
        args.set("name", FluentValue::from("a.png"));
        assert_eq!(format_in(&bundles, "action-open", None), "&Apri...");
        assert_eq!(format_in(&bundles, "action-quit", None), "E&xit", "missing → English");
        assert_eq!(
            format_in(&bundles, "rename-exists", Some(&args)),
            "Esiste già un file a.png."
        );
        assert_eq!(format_in(&bundles, "no-such-id", None), "no-such-id");
        let mut args = FluentArgs::new();
        for (k, v) in [("name", "x.jpg"), ("message", "bad"), ("code", "3")] {
            args.set(k, FluentValue::from(v));
        }
        assert_eq!(
            format_in(&bundles, "error-opening", Some(&args)),
            "Error occurred opening\nx.jpg\nbad (Error 3)"
        );
        assert_eq!(language_name(SOURCE), "English");
    }

    #[test]
    fn source_file_is_complete_and_valid() {
        let (_, src) = LANGUAGES.iter().find(|(c, _)| *c == SOURCE).expect("en.ftl embedded");
        let res = match FluentResource::try_new(src.to_string()) {
            Ok(r) => r,
            Err((_, e)) => panic!("en.ftl does not parse: {e:?}"),
        };
        let ids: std::collections::HashSet<String> = res
            .entries()
            .filter_map(|e| match e {
                fluent_syntax::ast::Entry::Message(m) => Some(m.id.name.to_string()),
                _ => None,
            })
            .collect();
        // every id the code asks for: t("…"), tf("…", …) and the per-action ids
        let mut used: Vec<String> = crate::actions::ACTIONS
            .iter()
            .map(|(a, _)| crate::actions::message_id(*a))
            .collect();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        // `t("id"` / `tf("id"` as a whole word (not `expect("…")`, `set("…")`)
        let re_like = |text: &str, prefix: &str, out: &mut Vec<String>| {
            for (i, _) in text.match_indices(prefix) {
                let before = text[..i].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let rest = &text[i + prefix.len()..];
                if let Some(end) = rest.find('"') {
                    let id = &rest[..end];
                    if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                        out.push(id.to_string());
                    }
                }
            }
        };
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("i18n.rs") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    for prefix in ["t(\"", "tf(\""] {
                        re_like(&text, prefix, &mut used);
                    }
                    // ids passed around as literals: menu titles, message box titles, Info labels
                    for family in ["menu-", "title-", "info-", "settings-", "markup-"] {
                        let mut found = vec![];
                        re_like(&text, &format!("\"{family}"), &mut found);
                        used.extend(found.into_iter().map(|rest| format!("{family}{rest}")));
                    }
                }
            }
        }
        let missing: Vec<&String> = used.iter().filter(|id| !ids.contains(*id)).collect();
        assert!(
            missing.is_empty(),
            "ids used in the code but missing in en.ftl: {missing:?}"
        );
        // and a translation never needs more than the source has
        set_language(SOURCE);
        assert_eq!(current(), SOURCE);
        assert_eq!(plain("Ope&n && Save"), "Open & Save");
    }
}
