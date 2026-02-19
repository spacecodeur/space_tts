use anyhow::Result;
use std::io::{BufReader, BufWriter, Write};

use crate::log::{debug, info};
use crate::protocol::{ClientMsg, ServerMsg, read_client_msg, write_server_msg};
use crate::transcribe::{LocalTranscriber, Transcriber};

pub fn run(model_path: &str, language: &str) -> Result<()> {
    info!("Server mode: loading model {model_path}...");

    let mut transcriber = LocalTranscriber::new(model_path, language)?;

    // Warm-up: transcribe 1s of silence to init GPU graph
    debug!("Warming up whisper...");
    let silence = vec![0i16; 16000];
    let _ = transcriber.transcribe(&silence);
    debug!("Warm-up complete.");

    // Send Ready on stdout
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    write_server_msg(&mut writer, &ServerMsg::Ready)?;
    writer.flush()?;

    info!("Server ready, waiting for audio segments...");

    // Read from stdin
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());

    loop {
        let msg = match read_client_msg(&mut reader) {
            Ok(msg) => msg,
            Err(e) => {
                // EOF or broken pipe = client disconnected
                let msg = format!("{e}");
                if msg.contains("unexpected end of file")
                    || msg.contains("UnexpectedEof")
                    || msg.contains("broken pipe")
                {
                    info!("Client disconnected, shutting down.");
                    break;
                }
                info!("Protocol error: {e}");
                break;
            }
        };

        match msg {
            ClientMsg::AudioSegment(samples) => {
                debug!(
                    "Received audio segment: {} samples ({:.0}ms)",
                    samples.len(),
                    samples.len() as f64 / 16.0
                );

                let response = match transcriber.transcribe(&samples) {
                    Ok(text) => ServerMsg::Text(text),
                    Err(e) => ServerMsg::Error(format!("{e}")),
                };

                write_server_msg(&mut writer, &response)?;
                writer.flush()?;
            }
        }
    }

    info!("Server shutdown complete.");
    Ok(())
}
