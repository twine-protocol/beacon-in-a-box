use anyhow::Result;
use std::{path::PathBuf, sync::Arc};
use twine::{
  prelude::*, twine_builder::RingSigner, twine_core::crypto::PublicKey,
};

use super::payload::*;

// TODO: consider using an enum to store the state of the assembler

pub struct PulseAssembler<S: Store + Resolver> {
  builder: TwineBuilder<PublicKey, RingSigner>,
  strand: Arc<Strand>,
  store: S,
  rng_path: String,
  prepared: Option<Twine>,
  next_randomness: Option<[u8; 64]>,
}

impl<S: Store + Resolver> PulseAssembler<S> {
  pub fn new(signer: RingSigner, strand: Arc<Strand>, store: S) -> Self {
    Self {
      builder: TwineBuilder::new(signer),
      strand,
      store,
      rng_path: "./randomness".to_string(),
      prepared: None,
      next_randomness: None,
    }
  }

  pub fn with_rng_path(mut self, rng_path: String) -> Self {
    self.rng_path = rng_path;
    self
  }

  fn rng_file(&self) -> PathBuf {
    PathBuf::from(&self.rng_path).join("rng.dat")
  }

  fn load_rng(&self) -> Result<Vec<u8>> {
    let rng = std::fs::read(&self.rng_file())?;
    if rng.len() != 64 {
      return Err(anyhow::anyhow!("Invalid RNG length {} bytes", rng.len()));
    }
    Ok(rng)
  }

  fn save_rng(&self, rng: &[u8; 64]) -> Result<()> {
    std::fs::write(self.rng_file(), rng)?;
    Ok(())
  }

  pub fn prepared(&self) -> Option<&Twine> {
    self.prepared.as_ref()
  }

  pub fn timestamp_of_publish(&self) -> Option<chrono::DateTime<chrono::Utc>> {
    self.prepared.as_ref().map(|twine| {
      twine
        .extract_payload::<RandomnessPayload>()
        .expect("payload")
        .timestamp()
    })
  }

  async fn latest(&mut self) -> Result<Option<Twine>> {
    let latest = match self.store.resolve_latest(&self.strand).await {
      Ok(latest) => Some(latest.unpack()),
      Err(e) => match e {
        ResolutionError::NotFound => None,
        _ => return Err(e.into()),
      },
    };

    Ok(latest)
  }

  pub async fn prepare_next(
    &mut self,
    next_randomness: &[u8; 64],
  ) -> Result<()> {
    use twine::twine_core::multihash_codetable::MultihashDigest;

    if let Some(prepared) = &self.prepared {
      return Err(anyhow::anyhow!(
        "Already prepared tixel hasn't been published: {}",
        prepared
      ));
    }

    let latest = self.latest().await?;

    let next = match latest {
      None => {
        let pre = self.strand.hasher().digest(next_randomness);
        // start the strand
        self.store.save(self.strand.clone()).await?;
        self
          .builder
          .build_first((*self.strand).clone())
          .payload(RandomnessPayload::new_start(pre)?)
          .done()?
      }
      Some(latest) => {
        let curr_randomness = self.load_rng()?;
        let pre = self.strand.hasher().digest(next_randomness);
        let payload =
          RandomnessPayload::from_rand(curr_randomness, pre, latest.tixel())?;
        self.builder.build_next(&latest).payload(payload).done()?
      }
    };

    self.prepared = Some(next);
    self.next_randomness = Some(*next_randomness);

    Ok(())
  }

  pub async fn publish(&mut self) -> Result<()> {
    let prepared = self
      .prepared
      .as_ref()
      .ok_or_else(|| anyhow::anyhow!("No prepared twine to commit"))?;

    self.store.save(prepared.clone()).await?;

    let next_randomness = self
      .next_randomness
      .as_ref()
      .ok_or_else(|| anyhow::anyhow!("No next randomness to commit"))?;

    self.save_rng(&next_randomness)?;

    self.prepared = None;
    self.next_randomness = None;

    Ok(())
  }
}
