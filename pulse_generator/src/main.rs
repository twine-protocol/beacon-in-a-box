use anyhow::Result;
use biab_utils::{handle_shutdown_signal, init_logger};
use chrono::{Duration, TimeDelta};
use std::{env, sync::Arc};
use tokio::{net::TcpStream, process::Command, sync::Notify};
use twine::{
  prelude::*,
  twine_core::{crypto::PublicKey, twine::CrossStitches},
};
mod pulse_assembler;
use pulse_assembler::*;
mod cid_str;
mod payload;
mod stitch_config;
mod timing;

#[tokio::main]
async fn main() -> Result<()> {
  init_logger();

  // Setup graceful shutdown
  let shutdown = Arc::new(Notify::new());
  tokio::spawn(handle_shutdown_signal(shutdown.clone()));

  let strand_path = env::var("STRAND_JSON_PATH")?;
  // let store = twine::twine_core::store::MemoryStore::new();
  let strand = retrieve_or_create_strand(get_signer()?, &strand_path).await?;
  let store =
    twine_sql_store::SqlStore::open("mysql://root:root@db/twine").await?;
  let assembler = PulseAssembler::new(get_signer()?, strand, store)
    .with_rng_path(env::var("RNG_STORAGE_PATH")?);

  assembler.init().await?;

  start_scheduler(assembler, shutdown).await
}

fn get_signer() -> Result<impl Signer<Key = PublicKey>> {
  let key_path = env::var("PRIVATE_KEY_PATH")?;
  let pem = std::fs::read_to_string(key_path)?;
  let signer = twine::twine_builder::RingSigner::from_pem(pem)?;
  Ok(signer)
}

async fn create_strand<S: Signer<Key = PublicKey>>(
  signer: S,
  strand_path: &str,
) -> Result<Arc<Strand>> {
  #[derive(Debug, serde::Deserialize)]
  struct StrandDetails {
    subspec: Option<String>,
    details: Ipld,
  }
  let builder = TwineBuilder::new(signer);
  let details = std::fs::read_to_string(env::var("STRAND_DETAILS_PATH")?)?;
  let details: StrandDetails =
    twine::twine_core::serde_ipld_dagjson::from_slice(details.as_bytes())?;

  log::info!("Creating new strand with details: {:?}", details);
  let strand = builder
    .build_strand()
    .subspec(details.subspec.unwrap_or("".to_string()))
    .details(details.details)
    .done()?;

  let json = strand.tagged_dag_json_pretty();
  std::fs::write(strand_path, json)?;
  log::info!("Strand created and saved to {}", strand_path);

  Ok(Arc::new(strand))
}

async fn retrieve_or_create_strand<S: Signer<Key = PublicKey>>(
  signer: S,
  strand_path: &str,
) -> Result<Arc<Strand>> {
  match std::fs::metadata(strand_path) {
    Ok(_) => {
      let json = std::fs::read_to_string(strand_path)?;
      let strand = Arc::new(Strand::from_tagged_dag_json(json)?);
      Ok(strand)
    }
    Err(e) => match e.kind() {
      std::io::ErrorKind::NotFound => create_strand(signer, strand_path).await,
      _ => Err(e.into()),
    },
  }
}

async fn start_scheduler(
  assembler: PulseAssembler<
    impl Store + Resolver + 'static,
    impl Signer<Key = PublicKey> + Send + Sync + 'static,
  >,
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
  assembler: &PulseAssembler<
    impl Store + Resolver + 'static,
    impl Signer<Key = PublicKey> + 'static,
  >,
) -> Result<()> {
  let lead_time_s = env::var("LEAD_TIME_SECONDS")
    .unwrap_or_else(|_| "10".to_string())
    .parse::<u64>()?;
  let lead_time = Duration::seconds(lead_time_s as i64);

  if assembler.needs_assembly().await {
    // refresh stitches within the time window
    let time_limit = assembler
      .next_state_in(lead_time + TimeDelta::seconds(1))
      .await;

    let prev_cross_stitches = assembler.previous_cross_stitches().await;
    let next_cross_stitches = match tokio::time::timeout(
      time_limit,
      refresh_stitches(prev_cross_stitches.clone()),
    )
    .await
    {
      Ok(res) => match res {
        Ok(cross_stitches) => cross_stitches,
        Err(e) => {
          log::error!("Failed to refresh stitches. {}", e);
          prev_cross_stitches
        }
      },
      Err(_) => {
        log::error!("Timed out refreshing stitches");
        prev_cross_stitches
      }
    };

    let sleep_time = assembler.next_state_in(lead_time).await;
    log::debug!("Sleeping for {:?}", sleep_time);
    tokio::time::sleep(sleep_time).await;
    assemble_job(assembler, next_cross_stitches).await?;
  } else if assembler.needs_publish().await {
    let sleep_time = assembler.next_state_in(lead_time).await;
    log::debug!("Sleeping for {:?}", sleep_time);
    tokio::time::sleep(sleep_time).await;
    publish_job(assembler).await?;
  } else {
    unreachable!();
  }
  Ok(())
}

async fn refresh_stitches(
  mut xstitches: CrossStitches,
) -> Result<CrossStitches> {
  let path = env::var("STITCH_CONFIG_PATH")?;
  let stitch_config = stitch_config::StitchConfig::load(&path)?;
  let stitch_resolver = stitch_config.get_resolver();
  let strands_to_entwine = stitch_config.strands();

  xstitches
    .stitches()
    .iter()
    .filter(|s| !strands_to_entwine.contains(&s.strand))
    .for_each(|s| {
      log::info!("Will not refresh stitch to external strand {}", s.strand);
    });

  for cid in strands_to_entwine {
    match xstitches
      .clone()
      .add_or_refresh(cid, &stitch_resolver)
      .await
    {
      Ok(updated) => {
        if xstitches.strand_is_stitched(cid) {
          log::info!("Refreshed stitch to external strand {}", cid);
        } else {
          log::info!("Added new stitch to external strand {}", cid);
        }
        xstitches = updated;
      }
      Err(e) => {
        log::error!("Error adding stitch to external strand {}: {}", cid, e);
      }
    }
  }

  Ok(xstitches)
}

async fn assemble_job(
  assembler: &PulseAssembler<
    impl Store + Resolver + 'static,
    impl Signer<Key = PublicKey> + 'static,
  >,
  next_cross_stitches: CrossStitches,
) -> Result<()> {
  let randomness = fetch_randomness().await?;
  let rand: [u8; 64] = randomness.as_slice().try_into()?;
  match assembler.prepare_next(&rand, next_cross_stitches).await {
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
  assembler: &PulseAssembler<
    impl Store + Resolver + 'static,
    impl Signer<Key = PublicKey> + 'static,
  >,
) -> Result<()> {
  match assembler.publish().await {
    Ok(latest) => {
      log::info!("Pulse ({}) published: {}", latest.index(), latest.tixel());

      // send a tcp message to the syncher
      let messenger = biab_utils::Messenger::new();
      if let Ok(mut stream) = TcpStream::connect("data_sync:5555").await {
        match messenger.send_text(&mut stream, "sync").await {
          Ok(_) => log::debug!("Notified data sync task"),
          Err(e) => {
            log::error!("Failed to send notification to data sync task: {}", e)
          }
        }
      }
    }
    Err(e) => {
      log::error!("Failed to publish pulse: {:?}", e);
      return Err(e);
    }
  }
  Ok(())
}

async fn fetch_randomness() -> Result<Vec<u8>> {
  log::info!("Fetching fresh randomness...");
  let rng_script =
    env::var("RNG_SCRIPT").unwrap_or_else(|_| "rng.py".to_string());
  let output = run_python_script(&rng_script).await?;
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
