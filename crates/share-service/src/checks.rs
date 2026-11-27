//! The automatic checks every version of every creation goes through before
//! anyone else can load it. A failed check holds that version back (only its
//! maker sees it, with the reason); a warning lets it through but puts it in
//! front of a person.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

/// What's in a creation's potluck.toml (the game's `creations::Manifest`).
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Toml {
    pub name: String,
    pub by: String,
    pub kind: String,
    pub about: String,
    pub main: String,
    pub version: u32,
    pub remix_of: Option<String>,
    pub licence: String,
    pub needs: BTreeMap<String, String>,
}

/// The API modules this service knows and the versions games have. Keep in
/// step with the modules in the game's script.rs.
pub const MODULES: &[(&str, &str)] = &[("world", "0.1"), ("tweak", "0.1")];
pub const KINDS: &[&str] = &["creature", "block", "tool", "game", "mod", "world", "thing"];
pub const LICENCES: &[&str] = &["CC-BY-4.0", "CC-BY-SA-4.0", "CC0-1.0"];

pub const MAX_FILES: usize = 64;
pub const MAX_FILE: usize = 128 * 1024;
pub const MAX_SIZE: usize = 256 * 1024;
const TEXT: &[&str] = &["luau", "lua", "toml", "md", "txt", "json"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Check {
    pub name: String,
    /// "pass", "warn" (a person looks) or "fail" (held back)
    pub result: String,
    pub note: String,
}

impl Check {
    fn new(name: &str, result: &str, note: impl Into<String>) -> Check {
        Check { name: name.into(), result: result.into(), note: note.into() }
    }
}

pub fn failed(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.result == "fail")
}

pub fn warned(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.result == "warn")
}

pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 80
        && !name.starts_with('/')
        && !name.contains("..")
        && !name.contains('\\')
        && name.chars().all(|c| c.is_alphanumeric() || "._-/".contains(c))
}

/// Words to look for in names, descriptions and the strings in code. A
/// line `word*` matches any word starting with it; `#` starts a comment.
#[derive(Default)]
pub struct Words {
    whole: HashSet<String>,
    starts: Vec<String>,
}

impl Words {
    pub fn parse(text: &str) -> Words {
        let mut w = Words::default();
        for line in text.lines() {
            let l = line.split('#').next().unwrap_or("").trim().to_lowercase();
            if l.is_empty() {
                continue;
            }
            match l.strip_suffix('*') {
                Some(p) => w.starts.push(fold(p)),
                None => {
                    w.whole.insert(fold(&l));
                }
            }
        }
        w
    }

    pub fn is_empty(&self) -> bool {
        self.whole.is_empty() && self.starts.is_empty()
    }

    /// The listed words found in some text, folding look-alike spellings.
    pub fn find(&self, text: &str) -> Vec<String> {
        let mut found = Vec::new();
        for word in text.split(|c: char| !(c.is_alphanumeric() || "@$".contains(c))) {
            if word.is_empty() {
                continue;
            }
            let f = fold(word);
            // the listed word itself, so a moderator sees which one matched
            let hit = if self.whole.contains(&f) { Some(f.clone()) } else { self.starts.iter().find(|s| f.starts_with(s.as_str())).cloned() };
            if let Some(hit) = hit.filter(|h| !found.contains(h)) {
                found.push(hit);
            }
        }
        found
    }
}

/// Lower case with number and symbol stand-ins turned back into letters and
/// runs of a letter squashed: "B4DDD w0rd" reads as "bad word".
fn fold(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        let c = match c {
            '0' => 'o',
            '1' | '!' => 'i',
            '3' => 'e',
            '4' | '@' => 'a',
            '5' | '$' => 's',
            '7' => 't',
            c => c,
        };
        if out.chars().last() != Some(c) {
            out.push(c);
        }
    }
    out
}

/// The strings and comments in Luau code: where words players see live.
pub fn strings_in(code: &str) -> String {
    let mut out = String::new();
    for t in crate::highlight::tokens(code) {
        if matches!(t.kind, crate::highlight::Kind::Str | crate::highlight::Kind::Comment) {
            out.push_str(t.text);
            out.push(' ');
        }
    }
    out
}

/// Whether Luau can compile it, without running anything.
fn compiles(name: &str, code: &str) -> Result<(), String> {
    let lua = mlua::Lua::new();
    let _ = lua.sandbox(true);
    match lua.load(code).set_name(format!("={name}")).into_function() {
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string().lines().next().unwrap_or("").replace("syntax error: ", "").trim().to_string()),
    }
}

fn version_ok(want: &str, have: &str) -> bool {
    let p = |s: &str| -> Option<(u32, u32)> {
        let mut it = s.trim().split('.');
        Some((it.next()?.parse().ok()?, it.next().unwrap_or("0").parse().ok()?))
    };
    match (p(want), p(have)) {
        // 0.x minors can break things, so they must match; from 1.0, any minor up to ours
        (Some((wm, wn)), Some((hm, hn))) => wm == hm && if wm == 0 { wn == hn } else { wn <= hn },
        _ => false,
    }
}

/// Runs every check on a creation's files.
pub fn run(files: &BTreeMap<String, String>, words: &Words, banned: &dyn Fn(&str) -> bool) -> (Option<Toml>, Vec<Check>) {
    let mut out = Vec::new();

    // Size and file names
    let total: usize = files.iter().map(|(k, v)| k.len() + v.len()).sum();
    let mut size_problems = Vec::new();
    if files.len() > MAX_FILES {
        size_problems.push(format!("it has {} files, and the most is {MAX_FILES}", files.len()));
    }
    if total > MAX_SIZE {
        size_problems.push(format!("it's {} KB, and the most is {} KB", total / 1024, MAX_SIZE / 1024));
    }
    for (name, text) in files {
        let ext = name.rsplit('.').next().unwrap_or("");
        if !safe_name(name) {
            size_problems.push(format!("{name} has a name that isn't allowed"));
        } else if !TEXT.contains(&ext) {
            size_problems.push(format!("{name} isn't a kind of file creations can have yet"));
        } else if text.len() > MAX_FILE {
            size_problems.push(format!("{name} is over {} KB", MAX_FILE / 1024));
        }
    }
    out.push(match size_problems.is_empty() {
        true => Check::new("Size", "pass", format!("{} files, {:.1} KB", files.len(), total as f64 / 1024.0)),
        false => Check::new("Size", "fail", size_problems.join("; ")),
    });

    // potluck.toml
    let toml = files.get("potluck.toml").map(|t| toml::from_str::<Toml>(t));
    let manifest = match toml {
        None => {
            out.push(Check::new("Made for Potluck", "fail", "there's no potluck.toml"));
            None
        }
        Some(Err(e)) => {
            out.push(Check::new("Made for Potluck", "fail", format!("potluck.toml has a mistake: {}", e.message())));
            None
        }
        Some(Ok(mut m)) => {
            if m.main.is_empty() {
                m.main = "main.luau".into();
            }
            if m.kind.is_empty() {
                m.kind = "creature".into();
            }
            if m.licence.is_empty() {
                m.licence = "CC-BY-4.0".into();
            }
            let mut problems = Vec::new();
            if m.name.trim().is_empty() || m.name.chars().count() > 48 {
                problems.push("its name must be 1 to 48 letters".to_string());
            }
            if m.about.chars().count() > 500 {
                problems.push("its about is over 500 letters".into());
            }
            if !files.contains_key(&m.main) {
                problems.push(format!("there's no {} to run", m.main));
            }
            if !KINDS.contains(&m.kind.as_str()) {
                problems.push(format!("kind \"{}\" isn't one of {}", m.kind, KINDS.join(", ")));
            }
            if !LICENCES.contains(&m.licence.as_str()) {
                problems.push(format!("licence \"{}\" isn't one of {}", m.licence, LICENCES.join(", ")));
            }
            out.push(match problems.is_empty() {
                true => Check::new("Made for Potluck", "pass", format!("a {} called {}", m.kind, m.name)),
                false => Check::new("Made for Potluck", "fail", problems.join("; ")),
            });
            Some(m)
        }
    };

    // The API it needs, and what the code asks for
    let mut api = Vec::new();
    if let Some(m) = &manifest {
        for (module, want) in &m.needs {
            match MODULES.iter().find(|(n, _)| n == module) {
                None => api.push(format!("there's no module called @potluck/{module}")),
                Some((_, have)) if !version_ok(want, have) => api.push(format!("@potluck/{module} {want} isn't in this version of Potluck ({have})")),
                _ => {}
            }
        }
    }
    for (name, text) in files.iter().filter(|(n, _)| n.ends_with(".luau") || n.ends_with(".lua")) {
        let mut rest = text.as_str();
        while let Some(i) = rest.find("@potluck/") {
            rest = &rest[i + 9..];
            let module: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !module.is_empty() && !MODULES.iter().any(|(n, _)| *n == module) {
                let note = format!("{name} asks for @potluck/{module}, which doesn't exist");
                if !api.contains(&note) {
                    api.push(note);
                }
            }
        }
    }
    let needs: Vec<String> = manifest.iter().flat_map(|m| m.needs.iter().map(|(k, v)| format!("{k} {v}"))).collect();
    out.push(match api.is_empty() {
        true => Check::new("API", "pass", if needs.is_empty() { "needs nothing special".to_string() } else { format!("needs {}", needs.join(", ")) }),
        false => Check::new("API", "fail", api.join("; ")),
    });

    // The code compiles
    let mut code_problems = Vec::new();
    for (name, text) in files.iter().filter(|(n, _)| n.ends_with(".luau") || n.ends_with(".lua")) {
        if name.starts_with("types/") {
            continue;
        }
        if let Err(e) = compiles(name, text) {
            code_problems.push(e);
        }
    }
    out.push(match code_problems.is_empty() {
        true => Check::new("Code", "pass", "every file compiles"),
        false => Check::new("Code", "fail", code_problems.join("; ")),
    });

    // Words: in its name and description they hold it back; in its code a person looks
    if words.is_empty() {
        out.push(Check::new("Words", "warn", "no word list is set up on this server, so a person checks"));
    } else {
        let mut shown = Vec::new();
        if let Some(m) = &manifest {
            shown.extend(words.find(&m.name));
            shown.extend(words.find(&m.about));
            shown.extend(words.find(&m.by));
        }
        let mut in_code = Vec::new();
        for (name, text) in files {
            let t = if name.ends_with(".luau") || name.ends_with(".lua") { strings_in(text) } else if name == "potluck.toml" || name == "AGENTS.md" || name.starts_with("types/") { String::new() } else { text.clone() };
            in_code.extend(words.find(&t));
        }
        out.push(if !shown.is_empty() {
            Check::new("Words", "fail", "its name or description has words the market doesn't allow")
        } else if !in_code.is_empty() {
            Check::new("Words", "warn", format!("its text has words a person should look at: {}", in_code.join(", ")))
        } else {
            Check::new("Words", "pass", "nothing found")
        });
    }

    // Files taken down before
    let bad: Vec<&String> = files.iter().filter(|(_, t)| banned(&crate::codes::sha256_hex(t.as_bytes()))).map(|(n, _)| n).collect();
    out.push(match bad.is_empty() {
        true => Check::new("Known files", "pass", "none taken down before"),
        false => Check::new("Known files", "fail", format!("{} was taken down before", bad.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))),
    });

    (manifest, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creation(main: &str) -> BTreeMap<String, String> {
        let mut f = BTreeMap::new();
        f.insert("potluck.toml".into(), "name = \"Hopper\"\nby = \"jo\"\nkind = \"creature\"\nmain = \"main.luau\"\n[needs]\nworld = \"0.1\"\n".into());
        f.insert("main.luau".into(), main.into());
        f
    }

    #[test]
    fn a_good_creation_passes() {
        let words = Words::parse("grumble\nsnark*\n");
        let (m, checks) = run(&creation("local world = require(\"@potluck/world\")\nprint(\"hi\")\n"), &words, &|_| false);
        assert_eq!(m.unwrap().name, "Hopper");
        assert!(checks.iter().all(|c| c.result == "pass"), "{checks:?}");
    }

    #[test]
    fn broken_code_and_unknown_modules_fail() {
        let (_, checks) = run(&creation("local x = = 2\nrequire(\"@potluck/files\")"), &Words::parse("x"), &|_| false);
        let code = checks.iter().find(|c| c.name == "Code").unwrap();
        assert_eq!(code.result, "fail");
        assert!(code.note.contains("main.luau"), "{}", code.note);
        assert_eq!(checks.iter().find(|c| c.name == "API").unwrap().result, "fail");
    }

    #[test]
    fn words_fold_look_alikes() {
        let w = Words::parse("grumble\nsnark*\n");
        assert_eq!(w.find("what a GRUMMBL3 day"), vec!["grumble"]);
        assert_eq!(w.find("snarky"), vec!["snark"]);
        assert!(w.find("grumbles").is_empty());
        let (_, checks) = run(&creation("print(\"so snarky\") -- fine"), &w, &|_| false);
        assert_eq!(checks.iter().find(|c| c.name == "Words").unwrap().result, "warn");
        let mut f = creation("print(1)");
        f.insert("potluck.toml".into(), "name = \"Grumble Bot\"\n".into());
        let (_, checks) = run(&f, &w, &|_| false);
        assert_eq!(checks.iter().find(|c| c.name == "Words").unwrap().result, "fail");
    }

    #[test]
    fn sizes_names_and_banned_files() {
        let mut f = creation("print(1)");
        f.insert("../escape.luau".into(), "x".into());
        f.insert("big.txt".into(), "a".repeat(MAX_FILE + 1));
        f.insert("picture.png".into(), "x".into());
        let (_, checks) = run(&f, &Words::parse("x"), &|_| false);
        let size = checks.iter().find(|c| c.name == "Size").unwrap();
        assert_eq!(size.result, "fail");
        assert!(size.note.contains("escape") && size.note.contains("big.txt") && size.note.contains("picture.png"), "{}", size.note);
        let bad = crate::codes::sha256_hex(b"print(1)");
        let (_, checks) = run(&creation("print(1)"), &Words::parse("x"), &|h| h == bad);
        assert_eq!(checks.iter().find(|c| c.name == "Known files").unwrap().result, "fail");
    }

    #[test]
    fn module_versions() {
        assert!(version_ok("0.1", "0.1"));
        assert!(!version_ok("0.2", "0.1"));
        assert!(!version_ok("0.0", "0.1"));
        assert!(version_ok("1.2", "1.4"));
        assert!(!version_ok("1.5", "1.4"));
        assert!(!version_ok("2.0", "1.4"));
    }
}
