//! Tab completion for the REPL: words from the prelude, imported namespaces
//! (`rg -> `), library names after `IMPORT`, and keywords.

use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

use crate::Session;
use crate::parser::KEYWORDS;

/// The completable words of one session, refreshed after every entry because
/// declarations and `IMPORT`s change them.
#[derive(Default)]
pub struct Completions {
    /// Words usable without a qualifier: prelude names, namespace names, keywords.
    global: Vec<String>,
    /// Per imported namespace, the words after `ns -> `.
    namespaced: Vec<(String, Vec<String>)>,
    /// Libraries `IMPORT` can load.
    libraries: Vec<String>,
}

/// The lowercase bare words of a signature, skipping `( ... )` slots:
/// `find (root: String) with (options: X)` gives `find`, `with`.
fn bare_words(signature: &str, out: &mut Vec<String>) {
    let mut depth = 0usize;
    let mut word = String::new();
    for c in signature.chars().chain(std::iter::once(' ')) {
        if depth == 0 && (c.is_alphanumeric() || c == '_') {
            word.push(c);
            continue;
        }
        if word.starts_with(|c: char| c.is_lowercase()) {
            out.push(std::mem::take(&mut word));
        }
        word.clear();
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
}

fn words_of(session: &Session, ns: Option<&str>) -> Vec<String> {
    let mut words = Vec::new();
    for s in session.symbols(ns).unwrap_or_default() {
        bare_words(&s.name, &mut words);
        for m in &s.members {
            bare_words(&m.signature, &mut words);
        }
    }
    words.sort();
    words.dedup();
    words
}

impl Completions {
    pub fn of(session: &Session) -> Completions {
        let mut global = words_of(session, None);
        let mut namespaced = Vec::new();
        for ns in session.namespaces().into_iter().skip(1) {
            namespaced.push((ns.clone(), words_of(session, Some(&ns))));
            global.push(ns);
        }
        global.extend(KEYWORDS.iter().map(|k| k.to_string()));
        global.sort();
        global.dedup();
        let libraries = crate::library::available(crate::library::user_dir().as_deref())
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        Completions {
            global,
            namespaced,
            libraries,
        }
    }

    /// Returns where the completed word starts in `line[..pos]` and its candidates.
    fn candidates(&self, line: &str, pos: usize) -> (usize, Vec<String>) {
        let before = &line[..pos];
        let start = before
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map_or(0, |i| {
                i + before[i..].chars().next().map_or(1, char::len_utf8)
            });
        let word = &before[start..];
        let keep = |pool: &[String], prefix: &str| -> Vec<String> {
            pool.iter()
                .filter(|w| w.starts_with(prefix))
                .cloned()
                .collect()
        };
        let keyword = before[..start].trim_end();
        if let Some(left) = keyword.strip_suffix(crate::parser::ARROW) {
            let left = left.trim_end();
            let ns = &left[left
                .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
                .map_or(0, |i| i + 1)..];
            let pool = self
                .namespaced
                .iter()
                .find(|(n, _)| n == ns)
                .map(|(_, w)| w);
            return (start, pool.map_or(vec![], |p| keep(p, word)));
        }
        if keyword.ends_with("IMPORT") || keyword.ends_with("IMPORT :") {
            return (start, keep(&self.libraries, word));
        }
        (start, keep(&self.global, word))
    }
}

/// The rustyline helper: completion only, the other hooks keep their defaults.
#[derive(Default)]
pub struct ReplHelper {
    pub completions: Completions,
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let (start, words) = self.completions.candidates(line, pos);
        let pairs = words
            .into_iter()
            .map(|w| Pair {
                display: w.clone(),
                replacement: w,
            })
            .collect();
        Ok((start, pairs))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;
}
impl Highlighter for ReplHelper {}
impl Validator for ReplHelper {}
impl Helper for ReplHelper {}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Completions {
        let mut s = Session::new();
        s.set_library_dir(None);
        s.run("IMPORT rg").unwrap();
        Completions::of(&s)
    }

    #[test]
    fn bare_words_skip_slots() {
        let mut w = vec![];
        bare_words("find (root: String) with (options: X)", &mut w);
        assert_eq!(w, ["find", "with"]);
    }

    #[test]
    fn completes_prelude_and_namespace_names() {
        let c = session();
        let (start, got) = c.candidates("  rea", 5);
        assert_eq!((start, got), (2, vec!["read".to_string()]));
        let (_, got) = c.candidates("r", 1);
        assert!(got.contains(&"rg".to_string()));
    }

    #[test]
    fn completes_inside_a_namespace() {
        let c = session();
        let (start, got) = c.candidates("rg -> sea", 9);
        assert_eq!(start, 6);
        assert_eq!(got, ["search"]);
        let (start, got) = c.candidates("LET r BE: rg->sea", 17);
        assert_eq!(start, 14);
        assert_eq!(got, ["search"]);
    }

    #[test]
    fn completes_library_names_after_import() {
        let c = session();
        let (start, got) = c.candidates("IMPORT g", 8);
        assert_eq!(start, 7);
        assert_eq!(got, ["git"]);
    }
}
