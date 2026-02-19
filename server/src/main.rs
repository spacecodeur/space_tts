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
        let models_dir = space_tts_common::models::default_models_dir();
        let models = space_tts_common::models::scan_models(&models_dir)?;
        for (name, path) in &models {
            println!("{name}\t{}", path.display());
        }
        return Ok(());
    }

    // Default: run as server (requires --model)
    let model = find_arg_value(&args, "--model")
        .ok_or_else(|| anyhow::anyhow!("Usage: space_tts_server --model <path> --language <lang>\n       space_tts_server --list-models"))?;
    let language = find_arg_value(&args, "--language").unwrap_or_else(|| "en".to_string());
    server::run(&model, &language)
}
