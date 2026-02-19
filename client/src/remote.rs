use anyhow::{Result, bail};
use std::io::{BufReader, BufWriter};
use std::process::{Child, Command, Stdio};

use space_tts_common::info;
use space_tts_common::protocol::{ClientMsg, ServerMsg, read_server_msg, write_client_msg};

pub trait Transcriber: Send {
    fn transcribe(&mut self, audio_i16: &[i16]) -> Result<String>;
}

pub struct RemoteTranscriber {
    child: Child,
    writer: BufWriter<std::process::ChildStdin>,
    reader: BufReader<std::process::ChildStdout>,
}

impl RemoteTranscriber {
    pub fn new(ssh_target: &str, remote_model_path: &str, language: &str) -> Result<Self> {
        info!("Connecting to {ssh_target}...");

        let mut child = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                ssh_target,
                "space_tts_server",
                "--model",
                remote_model_path,
                "--language",
                language,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // remote logs visible locally
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn SSH: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to open SSH stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to open SSH stdout"))?;

        let writer = BufWriter::new(stdin);
        let mut reader = BufReader::new(stdout);

        // Wait for Ready message from server
        let msg = read_server_msg(&mut reader)
            .map_err(|e| anyhow::anyhow!("Server did not send Ready: {e}"))?;

        match msg {
            ServerMsg::Ready => info!("Remote server ready."),
            ServerMsg::Error(e) => bail!("Remote server error during startup: {e}"),
            other => bail!("Unexpected message from server: {other:?}"),
        }

        Ok(Self {
            child,
            writer,
            reader,
        })
    }
}

impl Transcriber for RemoteTranscriber {
    fn transcribe(&mut self, audio_i16: &[i16]) -> Result<String> {
        write_client_msg(&mut self.writer, &ClientMsg::AudioSegment(audio_i16.to_vec()))?;

        match read_server_msg(&mut self.reader)? {
            ServerMsg::Text(text) => Ok(text),
            ServerMsg::Error(e) => bail!("Remote transcription error: {e}"),
            ServerMsg::Ready => bail!("Unexpected Ready message during transcription"),
        }
    }
}

impl Drop for RemoteTranscriber {
    fn drop(&mut self) {
        // Close stdin to signal EOF to the server
        drop(self.child.stdin.take());
        // Give the process a moment to exit, then kill
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }
}

/// Discover models available on a remote machine.
/// Executes `ssh <target> space_tts_server --list-models` and parses `name\tpath` lines.
pub fn list_remote_models(ssh_target: &str) -> Result<Vec<(String, String)>> {
    let output = Command::new("ssh")
        .args(["-o", "BatchMode=yes", ssh_target, "space_tts_server", "--list-models"])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run SSH: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Remote model listing failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let models: Vec<(String, String)> = stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let name = parts.next()?.to_string();
            let path = parts.next()?.to_string();
            if name.is_empty() || path.is_empty() {
                None
            } else {
                Some((name, path))
            }
        })
        .collect();

    Ok(models)
}
