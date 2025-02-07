use std::collections::HashSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use twine::prelude::{Cid, ResolverSetSeries};
use twine_http_store::v2::HttpStore;

use crate::cid_str::CidStr;

#[derive(Debug, Serialize, Deserialize)]
pub struct StitchEntry {
  pub strand: CidStr,
  pub resolver: String,
  #[serde(default)]
  pub stop: bool,
}

/// Expected yaml structure:
/// ```yaml
/// stitches:
///   - strand: bafyrei...
///     resolver: https://somewhere.com
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct StitchConfig {
  pub stitches: Vec<StitchEntry>,
}

impl StitchConfig {
  pub fn load(path: &str) -> Result<StitchConfig> {
    load_config(path)
  }

  pub fn get_resolver(&self) -> ResolverSetSeries<HttpStore> {
    // unique resolvers
    let uris = self
      .stitches
      .iter()
      .map(|entry| entry.resolver.clone())
      .collect::<HashSet<String>>();

    let resolvers = uris
      .iter()
      .map(|uri| {
        use twine_http_store::reqwest::Client;
        HttpStore::new(Client::new()).with_url(uri)
      })
      .collect();

    ResolverSetSeries::new(resolvers)
  }

  pub fn strands(&self) -> HashSet<Cid> {
    self
      .stitches
      .iter()
      .filter(|entry| !entry.stop)
      .map(|entry| entry.strand.clone().into())
      .collect()
  }
}

pub fn load_config(path: &str) -> Result<StitchConfig> {
  let file = std::fs::File::open(path)?;
  let reader = std::io::BufReader::new(file);
  let config = serde_yaml::from_reader(reader)?;
  Ok(config)
}
