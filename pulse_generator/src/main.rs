use anyhow::Result;
use biab_utils::{handle_shutdown_signal, init_logger};
use chrono::prelude::*;
use std::time::Duration;
use std::{env, sync::Arc};
use tokio::sync::Mutex;
use tokio::{process::Command, sync::Notify};
use tokio_cron_scheduler::{Job, JobScheduler};
use twine::prelude::*;
mod pulse_assembler;
use pulse_assembler::*;
mod payload;
mod tcp_server;

#[tokio::main]
async fn main() -> Result<()> {
  init_logger();

  // Setup graceful shutdown
  let shutdown = Arc::new(Notify::new());
  tokio::spawn(handle_shutdown_signal(shutdown.clone()));

  // Start TCP server
  // tokio::spawn(tcp_server::start_tcp_server(shutdown.clone()));

  let key_path = env::var("PRIVATE_KEY_PATH")?;
  let pem = std::fs::read_to_string(key_path)?;
  let signer = twine::twine_builder::RingSigner::from_pem(pem)?;
  let strand_path = env::var("STRAND_JSON_PATH")?;
  let json = std::fs::read_to_string(strand_path)?;
  let strand = Arc::new(Strand::from_tagged_dag_json(json)?);
  let store = twine::twine_core::store::MemoryStore::new();
  // load the strand information and key
  let assembler = PulseAssembler::new(signer, strand, store)
    .with_rng_path(env::var("RNG_STORAGE_PATH")?);

  start_scheduler(assembler, shutdown).await
}

// Periodic background task
async fn start_scheduler(
  assembler: PulseAssembler<impl Store + Resolver + 'static>,
  shutdown: Arc<Notify>,
) -> Result<()> {
  // Create a scheduler
  let mut scheduler = JobScheduler::new().await?;

  let assembler = Arc::new(Mutex::new(assembler));

  scheduler
    .add(Job::new_async(
      env::var("PREPARE_CRON_SCHEDULE")
        .unwrap_or_else(|_| "50 * * * * *".to_string()),
      move |_, scheduler: JobScheduler| {
        let assembler = assembler.clone();
        Box::pin(async move {
          match assemble_job(assembler.clone()).await {
            Ok(_) => {
              {
                let assembler = assembler.lock().await;
                log::debug!(
                  "Pulse prepared and ready for release: {}",
                  assembler.prepared().expect("prepared pulse")
                );
              }
              scheduler_publish_job(scheduler, assembler).await;
            }
            Err(e) => log::error!("Failed to assemble job: {:?}", e),
          }
        })
      },
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

async fn scheduler_publish_job(
  scheduler: JobScheduler,
  assembler: Arc<Mutex<PulseAssembler<impl Store + Resolver + 'static>>>,
) {
  let now = chrono::Utc::now();
  let duration = {
    let assembler = assembler.lock().await;
    let dt = assembler.timestamp_of_publish().expect("timestamp") - now;
    dt.to_std().unwrap_or(std::time::Duration::from_secs(0))
  };
  // round up to the nearest second
  let duration = Duration::from_secs(duration.as_secs() + 1);
  log::info!("Scheduling to publish in {:?}", duration);
  let ret = scheduler.add(
    Job::new_one_shot_async(duration, move |_, _| {
      let assembler = assembler.clone();
      Box::pin(async move {
        match publish_job(assembler.clone()).await {
          Ok(_) => log::debug!("Pulse published"),
          Err(e) => log::error!("Failed to publish pulse: {:?}", e),
        }
      })
    })
    .unwrap(),
  );

  if let Err(e) = ret.await {
    log::error!("Failed to schedule publish job: {:?}", e);
  }
}

async fn assemble_job(
  assembler: Arc<Mutex<PulseAssembler<impl Store + Resolver + 'static>>>,
) -> Result<()> {
  let randomness = fetch_randomness().await?;
  let rand: [u8; 64] = randomness.as_slice().try_into()?;
  let mut assembler = assembler.lock().await;
  assembler.prepare_next(&rand).await?;
  Ok(())
}

async fn publish_job(
  assembler: Arc<Mutex<PulseAssembler<impl Store + Resolver + 'static>>>,
) -> Result<()> {
  let mut assembler = assembler.lock().await;
  assembler.publish().await?;
  Ok(())
}

async fn fetch_randomness() -> Result<Vec<u8>> {
  log::info!("Fetching randomness...");
  let rng_script =
    env::var("RNG_SCRIPT").unwrap_or_else(|_| "rng.py".to_string());
  let output = run_python_script(&rng_script).await.unwrap();
  log::info!("Randomness: {:?}", output);
  Ok(output)
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
