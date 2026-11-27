//! Type checking creations as they're made. Luau's own type checker (its
//! new solver, from the luau-analysis crate, in Rust) reads a creation's
//! files with the API's types (`creations/types/potluck.d.luau`) standing
//! in for each `@potluck` module, and what it's sure of becomes a hint in
//! Look inside: a name the API doesn't have, a global that isn't one, the
//! wrong number of values, or a plain value of the wrong kind.
//!
//! It reports only those, in plain words. The checker finds much more in
//! code without type annotations (a creature's handlers have no type for
//! `self`, so it guesses one from how it's used), and none of that is sure
//! enough to put in front of someone making their first creature. Every
//! base creation checks with no hints; a test keeps it so.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use luau_analysis::diagnostics::error::types::{CountMismatchContext, TypeError, TypeErrorData, UnknownSymbolContext};
use luau_analysis::frontend::resolver::ModuleInfo;
use luau_analysis::{DefinitionEnvironment, FileResolver, Frontend, FrontendOptions, ModuleName, NullConfigResolver, SemanticModel, SourceCode, SourceCodeType, TypeCheckLimits};
use luau_common::BString;

use crate::creations::{Creation, Library, Origin, TYPES};

/// Each `@potluck` module, and the type in the types file that stands for
/// it (a client half's `net` is `ClientNet`).
const MODULE_TYPES: &[(&str, &str)] = &[("world", "World"), ("tweak", "Tweak"), ("game", "Game"), ("store", "Store"), ("input", "Input"), ("ui", "UI"), ("sound", "Sound"), ("net", "Net"), ("test", "Test")];

/// The name the types file is checked under.
const TYPES_MODULE: &str = "@potluck/types";

/// A file bigger than this isn't checked (a hint isn't worth a stall).
const MOST_BYTES: usize = 256 * 1024;

/// How long the checker may spend on one file.
const TIME_LIMIT: f64 = 2.0;

/// How many creations are checked at once.
const AT_ONCE: usize = 2;

/// One thing the checker is sure is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub file: String,
    /// Counted from 1.
    pub line: u32,
    pub text: String,
}

impl std::fmt::Display for Hint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} line {}: {}", self.file, self.line, self.text)
    }
}

/// A creation's files as the checker reads them.
struct Files<'a> {
    files: &'a BTreeMap<String, String>,
    /// The file being checked is the client half, whose `net` differs.
    client: bool,
}

impl Files<'_> {
    fn own(&self, name: &str) -> Option<&String> {
        let file = name.trim_start_matches("./");
        let file = if file.ends_with(".luau") { file.to_string() } else { format!("{file}.luau") };
        self.files.get(&file)
    }
}

impl FileResolver for Files<'_> {
    fn read_source(&mut self, name: &ModuleName) -> Option<SourceCode> {
        let name = String::from_utf8_lossy(name);
        let source = if name == TYPES_MODULE {
            TYPES.to_string()
        } else if let Some(m) = name.strip_prefix("@potluck/") {
            let m = m.split('@').next().unwrap_or_default();
            let ty = match MODULE_TYPES.iter().find(|(n, _)| *n == m)?.1 {
                "Net" if self.client => "ClientNet",
                t => t,
            };
            format!("local T = require(\"{TYPES_MODULE}\")\nreturn (nil :: any) :: T.{ty}\n")
        } else {
            self.own(&name)?.clone()
        };
        Some(SourceCode { source: BString::from(source), kind: SourceCodeType::Module })
    }

    fn resolve_module(&mut self, _context: Option<&ModuleInfo>, expr: luau_syntax::ast::Expression<'_>, _limits: &TypeCheckLimits) -> Option<ModuleInfo> {
        let name = String::from_utf8_lossy(expr.as_string()?.value.as_bytes()).to_string();
        let name = if name.starts_with('@') || name == TYPES_MODULE {
            name
        } else {
            // the creation's own files, as `require` finds them
            let file = name.trim_start_matches("./");
            if file.ends_with(".luau") { file.to_string() } else { format!("{file}.luau") }
        };
        Some(ModuleInfo { name: BString::from(name), optional: false })
    }
}

/// The hints for each of a creation's Luau files (not its types folder).
pub fn check(creation: &Creation) -> Vec<Hint> {
    let client = creation.manifest.client.as_deref();
    let mut hints = vec![];
    for (file, src) in &creation.files {
        if !file.ends_with(".luau") || file.contains('/') || src.len() > MOST_BYTES {
            continue;
        }
        let files = Files { files: &creation.files, client: Some(file.as_str()) == client };
        // the checker is new code: a panic in it mustn't reach the game
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check_file(files, file))) {
            Ok(h) => hints.extend(h),
            Err(_) => warn!("the type checker gave up on {}'s {file}", creation.id),
        }
    }
    hints.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    hints.dedup();
    hints
}

fn check_file(files: Files, file: &str) -> Vec<Hint> {
    let mut config = luau_config::Config::default();
    config.mode = luau_config::Mode::Strict;
    let options = FrontendOptions { retain_full_type_graphs: true, module_time_limit_sec: Some(TIME_LIMIT), ..FrontendOptions::default() };
    let mut fe = Frontend::new(files, NullConfigResolver { default_config: config }, options);
    if fe.register_builtin_globals(DefinitionEnvironment::Globals).is_err() {
        return vec![];
    }
    let name = BString::from(file);
    let Ok(result) = fe.check(&name, None) else { return vec![] };
    let Some(model) = fe.semantic_model(&name) else { return vec![] };
    result.errors.iter().filter(|e| e.module_name == name).filter_map(|e| hint(&model, e).map(|text| Hint { file: file.to_string(), line: e.location.begin.line + 1, text })).collect()
}

/// What a finding says, in plain words, if it's one the checker is sure of.
fn hint(model: &SemanticModel, e: &TypeError) -> Option<String> {
    let ty = |id| model.type_ref(id).map(|t| t.follow().to_string());
    match &e.data {
        TypeErrorData::UnknownSymbol(s) if s.context == UnknownSymbolContext::Binding => {
            let name = String::from_utf8_lossy(&s.name).to_string();
            let mut text = format!("there's nothing called {name} here");
            if let Some(near) = closest(&name, GLOBALS) {
                text += &format!(" (did you mean {near}?)");
            }
            Some(text)
        }
        TypeErrorData::UnknownProperty(p) => {
            // only a table the API or the standard library names: anything
            // else is a type the checker guessed
            let table = ty(p.table)?;
            let key = String::from_utf8_lossy(&p.key).to_string();
            let (module, keys) = known_table(&table)?;
            let mut text = format!("{module} has no {key}");
            if let Some(near) = closest(&key, &keys.iter().map(String::as_str).collect::<Vec<_>>()) {
                text += &format!(" (did you mean {module}.{near}?)");
            }
            Some(text)
        }
        TypeErrorData::CountMismatch(c) if c.context == CountMismatchContext::Arg => {
            let wants = match c.maximum {
                Some(most) if most != c.expected => format!("{} to {most} values", c.expected),
                _ if c.is_variadic => format!("at least {} {}", c.expected, values(c.expected)),
                _ => format!("{} {}", c.expected, values(c.expected)),
            };
            let given = if c.actual == 0 { "none".to_string() } else { c.actual.to_string() };
            Some(format!("this call takes {wants}, and it's given {given}"))
        }
        TypeErrorData::TypeMismatch(m) => {
            // only a plain value of the wrong kind: "two" for a number
            let given = ty(m.given_type)?;
            let wanted = ty(m.wanted_type)?;
            let plain = |t: &str| matches!(t, "string" | "number" | "boolean") || is_literal(t);
            if !plain(&given) || !simple(&wanted) {
                return None;
            }
            Some(format!("this should be {}, not {}", describe(&wanted), describe(&given)))
        }
        _ => None,
    }
}

fn values(n: usize) -> &'static str {
    if n == 1 { "value" } else { "values" }
}

/// A singleton type: `"middle"`, `true`, `false`.
fn is_literal(t: &str) -> bool {
    (t.starts_with('"') && t.ends_with('"') && t.len() >= 2) || t == "true" || t == "false"
}

/// A wanted type simple enough to name: a plain type, or a few choices of
/// words, maybe optional.
fn simple(t: &str) -> bool {
    let t = t.strip_suffix('?').map(|t| t.trim_start_matches('(').trim_end_matches(')')).unwrap_or(t);
    t.split(" | ").all(|p| matches!(p, "string" | "number" | "boolean" | "vector") || is_literal(p))
}

/// A type in words: "a number", or "one of "left" or "right"".
fn describe(t: &str) -> String {
    let t = t.strip_suffix('?').map(|t| t.trim_start_matches('(').trim_end_matches(')')).unwrap_or(t);
    let one = |p: &str| match p {
        "string" => "some text".to_string(),
        "number" => "a number".to_string(),
        "boolean" => "true or false".to_string(),
        "vector" => "a position".to_string(),
        p => p.to_string(),
    };
    let parts: Vec<&str> = t.split(" | ").collect();
    match parts.as_slice() {
        [one_only] => one(one_only),
        [rest @ .., last] => format!("one of {} or {}", rest.iter().map(|p| one(p)).collect::<Vec<_>>().join(", "), one(last)),
        [] => String::new(),
    }
}

/// The API's tables by their type's name, and the standard library's, with
/// what's in each.
fn known_table(name: &str) -> Option<(String, Vec<String>)> {
    if let Some(lib) = name.strip_prefix("typeof(").and_then(|n| n.strip_suffix(')')) {
        let keys = library_keys(lib)?;
        return Some((lib.to_string(), keys));
    }
    let module = match name {
        "World" => "world",
        "Tweak" => "tweak",
        "Game" => "game",
        "Store" => "store",
        "Input" => "input",
        "UI" => "ui",
        "Sound" => "sound",
        "Net" | "ClientNet" => "net",
        "Test" => "test",
        _ => return None,
    };
    Some((module.to_string(), type_keys(name)))
}

/// The fields of one of the types file's `export type X = { ... }`.
fn type_keys(name: &str) -> Vec<String> {
    let head = format!("export type {name} = {{");
    let Some(start) = TYPES.find(&head) else { return vec![] };
    TYPES[start + head.len()..]
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with('}'))
        .filter_map(|l| {
            let l = l.strip_prefix('\t')?;
            let (k, _) = l.split_once(':')?;
            k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_').then(|| k.to_string())
        })
        .collect()
}

/// What's in one of Luau's standard libraries, as a creation's sandbox has it.
fn library_keys(lib: &str) -> Option<Vec<String>> {
    static KEYS: std::sync::OnceLock<HashMap<String, Vec<String>>> = std::sync::OnceLock::new();
    let all = KEYS.get_or_init(|| {
        let lua = mlua::Lua::new();
        let mut all = HashMap::new();
        for lib in ["math", "string", "table", "vector", "bit32", "utf8", "buffer", "coroutine", "os", "debug"] {
            if let Ok(t) = lua.globals().get::<mlua::Table>(lib) {
                let keys = t.pairs::<String, mlua::Value>().filter_map(Result::ok).map(|(k, _)| k).collect();
                all.insert(lib.to_string(), keys);
            }
        }
        all
    });
    all.get(lib).cloned()
}

/// Globals a creation has, for "did you mean".
const GLOBALS: &[&str] = &[
    "print", "require", "math", "string", "table", "vector", "bit32", "utf8", "buffer", "coroutine", "pairs", "ipairs", "next", "select", "type", "typeof", "tostring", "tonumber", "pcall",
    "xpcall", "error", "assert", "setmetatable", "getmetatable", "rawget", "rawset", "rawequal", "rawlen", "unpack",
];

/// The nearest of `names` to `word`, if it's near enough to be a typo.
fn closest<'a>(word: &str, names: &[&'a str]) -> Option<&'a str> {
    let word = word.to_lowercase();
    names
        .iter()
        .map(|n| (strsim::damerau_levenshtein(&word, &n.to_lowercase()), *n))
        .filter(|(d, n)| *d > 0 && *d <= (n.len() / 3).clamp(1, 3))
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n)
}

/// The hints for the player's own creations, checked on a thread of their
/// own whenever their files change.
#[derive(Resource, Default)]
pub struct Hints {
    /// By creation: a hash of the files checked, and what was found.
    pub by: BTreeMap<String, (u64, Vec<Hint>)>,
    /// Checks still running, by creation, with the hash they're for.
    running: Arc<Mutex<BTreeMap<String, (u64, Option<Vec<Hint>>)>>>,
}

impl Hints {
    /// A creation's hints, once its files as they are now were checked.
    pub fn of(&self, c: &Creation) -> Option<&[Hint]> {
        self.by.get(&c.id).filter(|(h, _)| *h == files_hash(c)).map(|(_, v)| v.as_slice())
    }
}

/// A hash of a creation's Luau files.
pub fn files_hash(c: &Creation) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (name, src) in c.files.iter().filter(|(n, _)| n.ends_with(".luau")) {
        name.hash(&mut h);
        src.hash(&mut h);
    }
    h.finish()
}

/// Starts a check of each of the player's own creations whose files
/// changed, and takes in the checks that finished.
pub fn keep_checking(lib: Res<Library>, mut hints: ResMut<Hints>) {
    let done: Vec<(String, u64, Vec<Hint>)> = {
        let mut running = hints.running.lock().unwrap();
        let ids: Vec<String> = running.iter().filter(|(_, (_, v))| v.is_some()).map(|(id, _)| id.clone()).collect();
        ids.into_iter().filter_map(|id| running.remove(&id).and_then(|(h, v)| Some((id, h, v?)))).collect()
    };
    for (id, hash, found) in done {
        for h in &found {
            info!("{id}: {h}");
        }
        hints.by.insert(id, (hash, found));
    }
    for c in lib.list.values().filter(|c| matches!(c.origin, Origin::Mine)) {
        let hash = files_hash(c);
        let mut running = hints.running.lock().unwrap();
        if hints.by.get(&c.id).is_some_and(|(h, _)| *h == hash) || running.get(&c.id).is_some_and(|(h, _)| *h == hash) {
            continue;
        }
        // a few at a time: the rest wait for a later frame
        if running.values().filter(|(_, v)| v.is_none()).count() >= AT_ONCE {
            break;
        }
        running.insert(c.id.clone(), (hash, None));
        drop(running);
        let (running, c) = (hints.running.clone(), c.clone());
        std::thread::spawn(move || {
            let found = check(&c);
            if let Some(slot) = running.lock().unwrap().get_mut(&c.id).filter(|(h, _)| *h == hash) {
                slot.1 = Some(found);
            }
        });
    }
}
