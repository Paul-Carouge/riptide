//! Manual check of the self-update API. Never run it from `~/.local/bin`: it replaces ITSELF.
//!
//! `cargo run --example self_update -- check` asks GitHub for the latest Riptide release.
//! `<copy of this binary> install <base url>` installs `<base url>/riptide-x86_64-linux`,
//! verified against `<base url>/SHA256SUMS`, over the copy (e.g. served by
//! `python3 -m http.server`), then restarts it with the same arguments.

// The crate is a binary, so the engine is compiled into this example directly.
// The engine's `expect`s target the app binary, not this example.
#[allow(dead_code, unused_imports, unfulfilled_lint_expectations)]
#[path = "../src/engine/mod.rs"]
mod engine;

use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match arguments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check"] => engine::check_for_app_update().map(|release| {
            println!(
                "running {}, latest newer release: {release:?}",
                engine::RIPTIDE_VERSION
            )
        }),
        ["install", base_url] => {
            let release = engine::AppRelease {
                version: "9.9.9".to_string(),
                notes: String::new(),
                asset_url: format!("{base_url}/riptide-x86_64-linux"),
                checksums_url: format!("{base_url}/SHA256SUMS"),
            };
            engine::install_app_update(&release).and_then(|version| {
                println!("installed {version}");
                engine::restart_app()
            })
        }
        _ => {
            eprintln!("usage: self_update check | self_update install <base url>");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error} / {}", error.user_message_fr());
            ExitCode::FAILURE
        }
    }
}
