//! Reference docs (`docs/`), embedded at compile time so the installed binary
//! can show them with no filesystem access. `docs/` is the single source of
//! truth: drop a new `.md` in and it shows up in `aello docs` and the TUI
//! reader automatically — no code change needed.

use include_dir::{include_dir, Dir};

static DOCS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/docs");

/// A bundled doc. `slug` is the file stem (e.g. `concepts`), `title` the first
/// `# H1` (or a prettified slug), `body` the raw markdown.
pub struct Doc {
    pub slug: String,
    pub title: String,
    pub body: &'static str,
}

/// Preferred reading order; anything not listed sorts after, alphabetically.
const ORDER: &[&str] = &["concepts", "capabilities", "migrate"];

fn rank(slug: &str) -> usize {
    ORDER.iter().position(|s| *s == slug).unwrap_or(ORDER.len())
}

/// All bundled docs, in reading order.
pub fn all() -> Vec<Doc> {
    let mut docs: Vec<Doc> = DOCS
        .files()
        .filter(|f| f.path().extension().is_some_and(|e| e == "md"))
        .filter_map(|f| {
            let slug = f.path().file_stem()?.to_string_lossy().into_owned();
            let body = f.contents_utf8()?;
            let title = title_of(body).unwrap_or_else(|| prettify(&slug));
            Some(Doc { slug, title, body })
        })
        .collect();
    docs.sort_by(|a, b| rank(&a.slug).cmp(&rank(&b.slug)).then_with(|| a.slug.cmp(&b.slug)));
    docs
}

/// One doc by slug (case-insensitive), or None.
pub fn get(slug: &str) -> Option<Doc> {
    let s = slug.to_lowercase();
    all().into_iter().find(|d| d.slug.to_lowercase() == s)
}

/// Text of the first `# H1` heading, if any.
fn title_of(body: &str) -> Option<String> {
    body.lines().find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
}

/// "getting-started" → "Getting Started".
fn prettify(slug: &str) -> String {
    slug.split(['-', '_'])
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_the_known_docs() {
        let slugs: Vec<String> = all().into_iter().map(|d| d.slug).collect();
        for want in ["concepts", "capabilities", "migrate"] {
            assert!(slugs.contains(&want.to_string()), "missing doc '{want}'");
        }
    }

    #[test]
    fn concepts_sorts_first() {
        assert_eq!(all()[0].slug, "concepts");
    }

    #[test]
    fn title_comes_from_h1() {
        assert_eq!(get("concepts").unwrap().title, "Concepts");
        assert!(get("does-not-exist").is_none());
    }

    #[test]
    fn prettify_capitalizes_words() {
        assert_eq!(prettify("getting-started"), "Getting Started");
        assert_eq!(prettify("migrate"), "Migrate");
    }
}
