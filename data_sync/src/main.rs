use anyhow::Result;
use biab_utils::{handle_shutdown_signal, init_logger};
use std::{env, sync::Arc};
use tokio::{sync::Notify, time::sleep};
use twine::prelude::*;
use twine_http_store::v2::HttpStore;
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

  let remote_addr = env::var("REMOTE_STORE_ADDRESS")?;
  use twine_http_store::{reqwest::Client, v2};
  let client = Client::builder()
    .default_headers({
      use twine_http_store::reqwest::header::{
        HeaderMap, HeaderValue, AUTHORIZATION,
      };
      let mut headers = HeaderMap::new();
      let key = env::var("REMOTE_STORE_API_KEY")?;
      if !key.is_empty() {
        let value = format!("ApiKey {}", key);
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&value).unwrap());
      }
      headers
    })
    .build()?;
  let remote_store = v2::HttpStore::new(client).with_url(&remote_addr);

  worker(signals, store, remote_store).await
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

async fn worker(
  signals: Signals,
  store: SqlStore,
  remote_store: HttpStore,
) -> Result<()> {
  let worker = tokio::spawn(async move {
    loop {
      signals.start_sync.notified().await;

      tokio::select! {
        _ = signals.shutdown.notified() => {
          log::info!("Stopping tasks...");
          break;
        }
        res = start_sync(&store, &remote_store) => {
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

async fn start_sync(store: &SqlStore, remote_store: &HttpStore) -> Result<()> {
  use futures::TryStreamExt;
  log::debug!("Beginning sync...");
  store
    .strands()
    .await?
    .map_err(|e| anyhow::anyhow!(e))
    .and_then(|strand| async move {
      let (latest, remote_latest) = tokio::join!(
        store.resolve_latest(&strand),
        remote_store.resolve_latest(&strand)
      );

      let latest = match latest {
        Ok(latest) => latest,
        Err(ResolutionError::NotFound) => {
          log::error!("No latest tixel for strand: {}", strand.cid());
          return Ok(None);
        }
        Err(e) => {
          log::error!("Error resolving latest tixel: {}", e);
          return Ok(None);
        }
      };

      let remote_latest_index = match remote_latest {
        Ok(latest) => latest.index() + 1,
        Err(ResolutionError::NotFound) => 0,
        Err(e) => {
          log::error!("Error resolving remote latest tixel. Will attempt sync anyway.: {}", e);
          0
        }
      };

      if latest.index() <= remote_latest_index {
        log::debug!("No new tixels to sync for strand: {}", strand.cid());
        return Ok(None);
      }

      let range = AbsoluteRange::new(strand.cid(), remote_latest_index, latest.index());
      Ok(Some(range))
    })
    .try_filter_map(|x| async move { Ok(x) })
    .try_for_each(|range: AbsoluteRange| async move {
      log::debug!("Syncing range: {}", range);
      // if we're starting at zero, save the strand first
      if range.start == 0 {
        let strand = store.resolve_strand(range.strand_cid()).await?;
        remote_store.save(strand.unpack()).await?;
      }
      let stream = store.resolve_range(range).await?;
      // save them 1000 at a time
      stream
        .try_chunks(1000)
        .map_err(|e| anyhow::anyhow!(e))
        .try_for_each(|chunk| async {
          log::debug!("Saving chunk of {} tixels", chunk.len());
          remote_store.save_many(chunk).await?;
          Ok(())
        })
        .await?;
      Ok(())
    })
    .await?;

  log::debug!("Sync complete");
  Ok(())
}
