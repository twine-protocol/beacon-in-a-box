use anyhow::Result;
use biab_utils::{handle_shutdown_signal, init_logger};
use std::{env, sync::Arc};
use tokio::{sync::Notify, time::sleep};
use twine::prelude::*;
use twine_sql_store::SqlStore;

#[derive(Debug, Clone)]
struct Signals {
  pub shutdown: Arc<Notify>,
  pub start_sync: Arc<Notify>,
}

#[tokio::main]
async fn main() -> Result<()> {
  init_logger();

  // Setup graceful shutdown
  let shutdown = Arc::new(Notify::new());
  tokio::spawn(handle_shutdown_signal(shutdown.clone()));

  let signals = Signals {
    shutdown,
    start_sync: Arc::new(Notify::new()),
  };

  init_sync_scheduler(signals.clone());
  init_tcp_listener(signals.clone());

  let store =
    twine_sql_store::SqlStore::open("mysql://root:root@db/twine").await?;

  worker(signals, store).await
}

fn init_tcp_listener(signals: Signals) {
  // Load environment variables
  let addr: String =
    env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:5555".to_string());
  // Start TCP server
  let mut messages =
    biab_utils::start_tcp_server(addr, signals.shutdown.clone());

  // listen for messages from the TCP server
  tokio::spawn(async move {
    while let Some(message) = messages.recv().await {
      log::trace!("Received message: {:?}", message);
      if message.command == "sync" {
        signals.start_sync.notify_one();
      }
    }
  });
}

fn init_sync_scheduler(signals: Signals) {
  // Send a start sync signal every N seconds
  let sync_period_s = env::var("SYNC_PERIOD_SECONDS")
    .unwrap_or_else(|_| "30".to_string())
    .parse::<u64>()
    .expect("Invalid SYNC_PERIOD_SECONDS");

  let period = std::time::Duration::from_secs(sync_period_s);

  tokio::spawn(async move {
    loop {
      tokio::select! {
        _ = sleep(period) => {
          signals.start_sync.notify_one();
        }
        _ = signals.shutdown.notified() => {
          break;
        }
      }
    }
  });
}

async fn worker(signals: Signals, store: SqlStore) -> Result<()> {
  let worker = tokio::spawn(async move {
    loop {
      signals.start_sync.notified().await;

      tokio::select! {
        _ = signals.shutdown.notified() => {
          log::info!("Stopping tasks...");
          break;
        }
        res = start_sync(&store) => {
          if let Err(e) = res {
            log::error!("Error syncing: {}", e);
            sleep(std::time::Duration::from_secs(5)).await;
          }
        }
      }
    }
  });

  worker.await?;
  Ok(())
}

async fn start_sync(store: &SqlStore) -> Result<()> {
  use futures::{StreamExt, TryStreamExt};
  log::debug!("Beginning sync...");
  // simulate
  let strands = store.strands().await?.try_collect::<Vec<_>>().await?;
  let latests = futures::stream::iter(strands)
    .then(|strand| async move {
      let latest = store.resolve_latest(&strand).await?;
      Ok::<_, ResolutionError>((strand, latest.unpack()))
    })
    .try_collect::<Vec<_>>()
    .await?;
  latests.iter().for_each(|(strand, latest)| {
    log::debug!("Latest for {}: {}", strand.cid(), latest.index());
  });
  sleep(std::time::Duration::from_secs(5)).await;
  log::debug!("Sync complete");
  Ok(())
}
