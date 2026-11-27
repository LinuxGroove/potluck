//! potluck-share: the share service and web market behind Potluck's codes.
//! Players' games share creations to it, it keeps each one as a git
//! repository in Forgejo and its files by hash in object storage, checks
//! every version, and serves the market at its public address.

use std::sync::Arc;

use potluck_share::{checks, codes, config, db, forgejo, identity, market, store};

const HELP: &str = "potluck-share: Potluck's share service and web market

  potluck-share [--config FILE]                 run the service
  potluck-share [--config FILE] moderator NAME  make a maker (by handle) a moderator
  potluck-share [--config FILE] invite [N]      print N invite codes for the closed beta
  potluck-share [--config FILE] trust NAME      list a maker's shares without a look first
  potluck-share check FOLDER                    run the market's checks on a creation's folder

The config file is TOML (see deploy/market/share.toml); FORGEJO_TOKEN,
S3_ACCESS_KEY_ID and S3_SECRET_ACCESS_KEY can come from the environment.";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return;
    }
    let mut config_path = None;
    if let Some(i) = args.iter().position(|a| a == "--config") {
        if i + 1 < args.len() {
            config_path = Some(args[i + 1].clone());
            args.drain(i..=i + 1);
        }
    } else if let Some(i) = args.iter().position(|a| a.starts_with("--config=")) {
        config_path = Some(args[i]["--config=".len()..].to_string());
        args.remove(i);
    }
    if args.first().map(|s| s.as_str()) == Some("check") {
        std::process::exit(check_folder(args.get(1).map(|s| s.as_str()).unwrap_or(".")));
    }
    let cfg = match config::Config::load(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("potluck-share: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&cfg.data) {
        eprintln!("potluck-share: couldn't make {}: {e}", cfg.data.display());
        std::process::exit(2);
    }
    let conn = match db::open(&cfg.data.join("market.db")) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("potluck-share: the index: {e}");
            std::process::exit(2);
        }
    };
    match args.first().map(|s| s.as_str()) {
        Some("moderator") | Some("trust") => {
            let Some(handle) = args.get(1) else {
                eprintln!("which maker? give their handle");
                std::process::exit(2);
            };
            let col = if args[0] == "moderator" { "moderator = 1, beta = 1" } else { "trusted = 1" };
            match conn.execute(&format!("UPDATE accounts SET {col} WHERE handle = ?1 COLLATE NOCASE"), [handle.trim_start_matches('@')]) {
                Ok(1) => println!("done: @{handle}"),
                _ => {
                    eprintln!("there's no maker @{handle} yet: they appear once they've shared or opened the market from the game");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some("invite") => {
            let n: usize = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(1);
            for _ in 0..n.min(200) {
                let code = codes::new_code();
                let _ = conn.execute("INSERT INTO invites (code, made_by, created) VALUES (?1, 'cli', ?2)", rusqlite::params![code, db::now()]);
                println!("{}", codes::pretty(&code));
            }
            return;
        }
        Some(other) => {
            eprintln!("potluck-share: there's no command {other:?}\n\n{HELP}");
            std::process::exit(2);
        }
        None => {}
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a tokio runtime");
    rt.block_on(serve(cfg, conn));
}

async fn serve(cfg: config::Config, conn: rusqlite::Connection) {
    let http = reqwest::Client::builder().user_agent(concat!("potluck-share/", env!("CARGO_PKG_VERSION"))).timeout(std::time::Duration::from_secs(30)).build().expect("an http client");
    let store = match store::Store::new(&cfg, http.clone()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("potluck-share: {e}");
            std::process::exit(2);
        }
    };
    let key = codes::signing_key(&cfg.data).expect("the signing key");
    let words = match &cfg.words {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(t) => checks::Words::parse(&t),
            Err(e) => {
                eprintln!("potluck-share: couldn't read the word list {}: {e}", p.display());
                std::process::exit(2);
            }
        },
        None => {
            eprintln!("potluck-share: no word list (words = ...): every share to Everyone waits for a person");
            checks::Words::default()
        }
    };
    if cfg.forgejo.token.is_empty() {
        eprintln!("potluck-share: no Forgejo token (forgejo.token or FORGEJO_TOKEN)");
        std::process::exit(2);
    }
    if cfg.dev_accounts {
        eprintln!("potluck-share: dev_accounts is on: anyone can sign in as anyone. Only for tests on this machine");
    }
    let git = forgejo::Forgejo::new(&cfg.forgejo, http.clone());
    let ids = identity::Identity::new(&cfg.nakama, cfg.dev_accounts, http.clone());
    let listen = cfg.listen.clone();
    let app = Arc::new(market::App::new(cfg, conn, git, store, ids, key, words));

    // The base game's creations go up as soon as Forgejo answers.
    let seeding = app.clone();
    tokio::spawn(async move {
        for attempt in 0..60 {
            match seeding.git.version().await {
                Ok(v) => {
                    println!("potluck-share: git hosting is Forgejo {v}");
                    match seeding.seed_base().await {
                        Ok(()) => return,
                        Err(e) => eprintln!("potluck-share: the base creations: {}", e.message),
                    }
                }
                Err(e) if attempt % 10 == 0 => eprintln!("potluck-share: waiting for git hosting ({e})"),
                Err(_) => {}
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    let router = potluck_share::router(app);
    let listener = match tokio::net::TcpListener::bind(&listen).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("potluck-share: couldn't listen on {listen}: {e}");
            std::process::exit(2);
        }
    };
    println!("potluck-share: listening on {listen}");
    let stop = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let stop = async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signals");
        tokio::select! { _ = stop => {}, _ = term.recv() => {} }
    };
    if let Err(e) = axum::serve(listener, router).with_graceful_shutdown(stop).await {
        eprintln!("potluck-share: {e}");
    }
}

/// `potluck-share check FOLDER`: the market's checks on a creation, so a
/// maker (or an AI helper) can see what the market will say before sharing.
fn check_folder(dir: &str) -> i32 {
    let mut files = std::collections::BTreeMap::new();
    let mut stack = vec![(std::path::PathBuf::from(dir), String::new())];
    while let Some((d, prefix)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = format!("{prefix}{}", e.file_name().to_string_lossy());
            if name.starts_with('.') {
                continue;
            }
            if e.path().is_dir() {
                stack.push((e.path(), format!("{name}/")));
            } else if let Ok(t) = std::fs::read_to_string(e.path()) {
                files.insert(name, t);
            }
        }
    }
    let words = std::env::var("POTLUCK_WORDS").ok().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| checks::Words::parse(&t)).unwrap_or_default();
    let (_, results) = checks::run(&files, &words, &|_| false);
    for c in &results {
        println!("{:4}  {}: {}", c.result, c.name, c.note);
    }
    if checks::failed(&results) { 1 } else { 0 }
}
