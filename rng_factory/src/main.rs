use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use biab_utils::{handle_shutdown_signal, init_logger};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::Notify;
use tokio::time::interval;

#[tokio::main]
async fn main() -> Result<()> {
  let rng_script = std::env::var("RNG_SCRIPT_PATH").unwrap_or_else(|_| "rng.py".to_string());
  let shutdown = Arc::new(Notify::new());
  let mut stream = TcpStream::connect("generator:5555").await?;
  let messenger = biab_utils::Messenger::new();

  init_logger();
  tokio::spawn(handle_shutdown_signal(shutdown.clone()));

  let mut interval = interval(Duration::from_secs(5));

  loop {
    tokio::select! {
      _ = interval.tick() => {
        log::info!("Fetching randomness...");
        let output = run_python_script(&rng_script).await?;
        // Simulate work
        messenger.send_delivery(&mut stream, "randomness", &output).await?;
      }
      _ = shutdown.notified() => {
        log::info!("Stopping tasks...");
        break;
      }
    }
  }

  Ok(())
}

async fn run_python_script(script_path: &str) -> Result<Vec<u8>> {
  let output = Command::new("python3").arg(script_path).output().await?;
  if !output.status.success() {
    return Err(anyhow::anyhow!(
      "Failed to run python script: {}",
      String::from_utf8_lossy(&output.stderr)
    ));
  }

  Ok(output.stdout)
}
