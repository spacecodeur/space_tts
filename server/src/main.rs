mod server;
mod transcribe;

use anyhow::Result;

fn find_arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Parse --debug flag
    if args.iter().any(|a| a == "--debug") {
        space_tts_common::log::set_debug(true);
    }

    // --list-models: print local models and exit
    if args.iter().any(|a| a == "--list-models") {
        use std::io::IsTerminal;
        let models_dir = space_tts_common::models::default_models_dir();
        let models = space_tts_common::models::scan_models(&models_dir)?;
        if std::io::stdout().is_terminal() {
            // Interactive: human-friendly output
            if models.is_empty() {
                println!("No models found in {}", models_dir.display());
            } else {
                println!("Available models ({}):\n", models_dir.display());
                for (name, _) in &models {
                    println!("  space_tts_server --model {name} --language fr");
                }
            }
        } else {
            // Piped (e.g. SSH): machine-parseable name\tpath
            for (name, path) in &models {
                println!("{name}\t{}", path.display());
            }
        }
        return Ok(());
    }

    // Default: run as server (requires --model)
    let model_arg = find_arg_value(&args, "--model")
        .ok_or_else(|| anyhow::anyhow!("Usage: space_tts_server --model <name> --language <lang>\n       space_tts_server --list-models"))?;
    let model = space_tts_common::models::resolve_model_path(&model_arg);
    let language = find_arg_value(&args, "--language").unwrap_or_else(|| "en".to_string());
    server::run(&model.to_string_lossy(), &language)
}
