use anyhow::Result;
use biab_utils::{handle_shutdown_signal, init_logger};
use std::{env, sync::Arc};
use tokio::{process::Command, sync::Notify};
use tokio_cron_scheduler::{Job, JobScheduler};

mod tcp_server;

#[tokio::main]
async fn main() -> Result<()> {
  init_logger();

  // Setup graceful shutdown
  let shutdown = Arc::new(Notify::new());
  tokio::spawn(handle_shutdown_signal(shutdown.clone()));

  // Start TCP server
  // tokio::spawn(tcp_server::start_tcp_server(shutdown.clone()));

  start_scheduler(shutdown).await
}

// Periodic background task
async fn start_scheduler(shutdown: Arc<Notify>) -> Result<()> {
  // Create a scheduler
  let mut scheduler = JobScheduler::new().await?;

  scheduler
    .add(Job::new_async(
      env::var("PREPARE_CRON_SCHEDULE").unwrap_or_else(|_| "*/5 * * * * *".to_string()),
      |_, _| Box::pin(fetch_randomness()),
    )?)
    .await?;

  // Run the scheduler
  scheduler.start().await?;

  // Wait for shutdown signal
  shutdown.notified().await;
  log::info!("Stopping tasks...");
  scheduler.shutdown().await?;

  Ok(())
}

async fn fetch_randomness() {
  log::info!("Fetching randomness...");
  let rng_script = env::var("RNG_SCRIPT").unwrap_or_else(|_| "rng.py".to_string());
  let output = run_python_script(&rng_script).await.unwrap();
  log::info!("Randomness: {:?}", output);
}

async fn run_python_script(command: &str) -> Result<Vec<u8>> {
  let parts: Vec<&str> = command.split_whitespace().collect();
  let mut cmd = Command::new(parts[0]);
  for part in &parts[1..] {
    cmd.arg(part);
  }
  let output = cmd.output().await?;
  if !output.status.success() {
    return Err(anyhow::anyhow!(
      "Failed to run python script: {}",
      String::from_utf8_lossy(&output.stderr)
    ));
  }

  Ok(output.stdout)
}
