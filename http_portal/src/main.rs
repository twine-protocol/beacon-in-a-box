use anyhow::Result;
use biab_utils::{handle_shutdown_signal, init_logger};
use std::{env, sync::Arc};
use tokio::sync::Notify;
use twine::prelude::*;
use twine_sql_store::SqlStore;
use warp::Filter;

mod dag_json;

#[tokio::main]
async fn main() -> Result<()> {
  if env::var_os("RUST_LOG").is_none() {
    // Set `RUST_LOG=todos=debug` to see debug logs,
    // this only shows access logs.
    env::set_var("RUST_LOG", "todos=info");
  }

  init_logger();

  // Setup graceful shutdown
  let shutdown = Arc::new(Notify::new());
  tokio::spawn(handle_shutdown_signal(shutdown.clone()));

  let port = env::var("PORT")
    .unwrap_or("80".into())
    .parse::<u16>()
    .expect("PORT must be a number");

  let store = SqlStore::open("mysql://root:root@db/twine").await?;

  // view access logs with RUST_LOG=api
  let api = filters::api(store).with(warp::log("api"));

  tokio::select! {
    _ = warp::serve(api).run(([0, 0, 0, 0], port)) => {}
    _ = shutdown.notified() => {
      log::info!("Shutting down...");
    }
  };

  Ok(())
}

mod filters {
  use super::*;
  use serde::Deserialize;
  use std::sync::Arc;
  use warp::reply;

  // GET / -> all strands
  // GET /:query -> parse the AnyQuery and return the result
  // GET /:query?full -> also include the strand in the result

  #[derive(Debug, Deserialize)]
  struct QueryParams {
    full: bool,
  }

  pub fn api(
    store: SqlStore,
  ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
  {
    let store = Arc::new(store);
    list_strands(store.clone()).or(query(store)).recover(
      |err: warp::Rejection| async move {
        let res = match err.find::<handlers::HttpError>() {
          Some(handlers::HttpError(e)) => match e {
            ResolutionError::NotFound => reply::with_status(
              reply::json(&models::AnyResult::Error {
                error: "not found".to_string(),
              }),
              warp::http::StatusCode::NOT_FOUND,
            ),
            _ => reply::with_status(
              reply::json(&models::AnyResult::Error {
                error: e.to_string(),
              }),
              warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ),
          },
          None => return Err(err),
        };
        Ok(res)
      },
    )
  }

  fn list_strands(
    store: Arc<SqlStore>,
  ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
  {
    warp::path::end()
      .and(with_store(store))
      .and_then(|store| async move {
        let res = handlers::list_strands(store).await;
        match res {
          Ok(reply) => Ok(reply),
          Err(err) => Err(warp::reject::custom(err)),
        }
      })
  }

  fn query(
    store: Arc<SqlStore>,
  ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
  {
    warp::path::param()
      .and(with_store(store))
      .and(warp::query::<QueryParams>())
      .and_then(|query, store, params: QueryParams| async move {
        let res = handlers::query(query, store, params.full).await;
        match res {
          Ok(reply) => Ok(reply),
          Err(err) => Err(warp::reject::custom(err)),
        }
      })
  }

  fn with_store(
    store: Arc<SqlStore>,
  ) -> impl Filter<Extract = (Arc<SqlStore>,), Error = std::convert::Infallible>
       + Clone {
    warp::any().map(move || store.clone())
  }
}

mod handlers {
  use std::sync::Arc;

  use super::*;
  use futures::TryStreamExt;

  #[derive(Debug)]
  pub struct HttpError(pub ResolutionError);
  impl From<ResolutionError> for HttpError {
    fn from(e: ResolutionError) -> Self {
      HttpError(e)
    }
  }
  impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
      write!(f, "{}", self.0)
    }
  }
  impl std::error::Error for HttpError {}
  impl warp::reject::Reject for HttpError {}

  pub async fn query(
    q: AnyQuery,
    store: Arc<SqlStore>,
    full: bool,
  ) -> Result<impl warp::Reply, HttpError> {
    let result = match q {
      AnyQuery::Strand(strand_cid) => {
        let strand = store.resolve_strand(&strand_cid).await?;
        models::AnyResult::Strands {
          items: vec![(*strand.unpack()).clone().into()],
        }
      }
      AnyQuery::One(query) => {
        let twine = store.resolve(query).await?;
        let strand = if full {
          let strand = (*twine.strand()).clone().into();
          Some(strand)
        } else {
          None
        };
        models::AnyResult::Tixels {
          items: vec![(*twine.unpack()).clone().into()],
          strand,
        }
      }
      AnyQuery::Many(range) => {
        let tixels: Vec<_> =
          store.resolve_range(range).await?.try_collect().await?;
        let strand = if full {
          let strand = (*tixels[0].strand()).clone().into();
          Some(strand)
        } else {
          None
        };
        models::AnyResult::Tixels {
          items: tixels.into_iter().map(|t| (*t).clone().into()).collect(),
          strand,
        }
      }
    };
    Ok(warp::reply::json(&result))
  }

  pub async fn list_strands(
    store: Arc<SqlStore>,
  ) -> Result<impl warp::Reply, HttpError> {
    let strands: Vec<_> = store.strands().await?.try_collect().await?;
    let result = models::AnyResult::Strands {
      items: strands.into_iter().map(|s| (*s).clone().into()).collect(),
    };
    Ok(warp::reply::json(&result))
  }
}

mod models {
  use super::*;
  use serde::{Deserialize, Serialize};
  use twine::twine_core::twine::Tagged;

  // The api can return a json object with an "items" array
  // which possibly contains a "strand" object containing the owning strand
  // If it's an error, it returns an object with an "error" key
  #[derive(Debug, Serialize, Deserialize)]
  #[serde(untagged)]
  pub enum AnyResult {
    Tixels {
      #[serde(with = "crate::dag_json")]
      items: Vec<Tagged<Tixel>>,
      #[serde(with = "crate::dag_json")]
      #[serde(skip_serializing_if = "Option::is_none")]
      strand: Option<Tagged<Strand>>,
    },
    Strands {
      #[serde(with = "crate::dag_json")]
      items: Vec<Tagged<Strand>>,
    },
    Error {
      error: String,
    },
  }
}
