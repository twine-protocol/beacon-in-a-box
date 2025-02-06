use anyhow::Result;
use biab_utils::{handle_shutdown_signal, init_logger};
use chrono::Duration;
use std::{env, sync::Arc};
use tokio::{process::Command, sync::Notify};
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
  // let store = twine::twine_core::store::MemoryStore::new();
  let store =
    twine_sql_store::SqlStore::open("mysql://root:root@db/twine").await?;
  let assembler = PulseAssembler::new(signer, strand, store)
    .with_rng_path(env::var("RNG_STORAGE_PATH")?);

  assembler.init().await?;

  start_scheduler(assembler, shutdown).await
}

async fn start_scheduler(
  assembler: PulseAssembler<impl Store + Resolver + 'static>,
  shutdown: Arc<Notify>,
) -> Result<()> {
  let worker = tokio::spawn(async move {
    loop {
      tokio::select! {
        _ = shutdown.notified() => {
          log::info!("Stopping tasks...");
          break;
        }
        res = advance(&assembler) => {
          if let Err(e) = res {
            log::error!("Error advancing: {}", e);
            break;
          }
        }
      }
    }
  });

  worker.await?;
  Ok(())
}

async fn advance(
  assembler: &PulseAssembler<impl Store + Resolver + 'static>,
) -> Result<()> {
  let seconds = env::var("LEAD_TIME_SECONDS")
    .unwrap_or_else(|_| "10".to_string())
    .parse::<u64>()?;

  let sleep_time = assembler
    .next_state_in(Duration::seconds(seconds as i64))
    .await;

  log::debug!("Sleeping for {:?}", sleep_time);
  tokio::time::sleep(sleep_time).await;

  if assembler.needs_assembly().await {
    assemble_job(assembler).await?;
  } else if assembler.needs_publish().await {
    publish_job(assembler).await?;
  } else {
    unreachable!();
  }
  Ok(())
}

async fn assemble_job(
  assembler: &PulseAssembler<impl Store + Resolver + 'static>,
) -> Result<()> {
  let randomness = fetch_randomness().await?;
  let rand: [u8; 64] = randomness.as_slice().try_into()?;
  match assembler.prepare_next(&rand).await {
    Ok(_) => {
      log::info!(
        "Pulse {} prepared and ready for release",
        assembler.prepared().await.expect("prepared pulse").index()
      );
      Ok(())
    }
    Err(e) => {
      log::error!("Failed to prepare pulse: {:?}", e);
      Err(e)
    }
  }
}

async fn publish_job(
  assembler: &PulseAssembler<impl Store + Resolver + 'static>,
) -> Result<()> {
  match assembler.publish().await {
    Ok(latest) => {
      log::info!("Pulse ({}) published: {}", latest.index(), latest.tixel());
    }
    Err(e) => {
      log::error!("Failed to publish pulse: {:?}", e);
      return Err(e);
    }
  }
  Ok(())
}

async fn fetch_randomness() -> Result<Vec<u8>> {
  log::info!("Fetching randomness...");
  let rng_script =
    env::var("RNG_SCRIPT").unwrap_or_else(|_| "rng.py".to_string());
  let output = run_python_script(&rng_script).await?;
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
