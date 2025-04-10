use anyhow::Result;
use chrono::Duration;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use twine_protocol::{
  prelude::*,
  twine_lib::{crypto::PublicKey, twine::CrossStitches},
};

use twine_spec_rng::{PayloadBuilder, RandomnessPayload, RngStrandDetails};

#[derive(Debug, Clone)]
pub enum AssemblyState {
  BeginStrand(Duration),
  Prepared { rand: [u8; 64], prepared: Twine },
  Released { rand: [u8; 64], latest: Twine },
}

impl AssemblyState {
  pub fn new_from_scratch(period: Duration) -> Self {
    AssemblyState::BeginStrand(period)
  }

  pub fn new_from_latest(latest: Twine, rand: [u8; 64]) -> Self {
    AssemblyState::Released { latest, rand }
  }

  pub fn time_till_state_change(
    &self,
    lead_time: Duration,
  ) -> std::time::Duration {
    let now = chrono::Utc::now();
    match self {
      AssemblyState::BeginStrand(period) => {
        let next_ts = crate::timing::next_truncated_time(*period);
        let next_time = next_ts - lead_time;
        next_time
          .signed_duration_since(now)
          .to_std()
          .unwrap_or(std::time::Duration::from_secs(0))
      }
      AssemblyState::Prepared { prepared, .. } => {
        // if prepared, we wait until the prepared timestamp
        prepared
          .extract_payload::<RandomnessPayload>()
          .expect("payload")
          .timestamp()
          .signed_duration_since(now)
          .to_std()
          .unwrap_or(std::time::Duration::from_secs(0))
      }
      AssemblyState::Released { latest, .. } => {
        // if awaiting next assembly...
        let prev_ts = latest
          .extract_payload::<RandomnessPayload>()
          .expect("payload")
          .timestamp();

        let period = latest
          .strand()
          .extract_details::<RngStrandDetails>()
          .unwrap()
          .period;

        let next_ts = crate::timing::next_pulse_timestamp(prev_ts, period);
        let next_time = next_ts - lead_time;
        next_time
          .signed_duration_since(now)
          .to_std()
          .unwrap_or(std::time::Duration::from_secs(0))
      }
    }
  }
}

pub struct PulseAssembler<S: Store + Resolver, G: Signer<Key = PublicKey>> {
  builder: TwineBuilder<2, G>,
  strand: Strand,
  period: Duration,
  store: S,
  rng_path: String,
  state: Arc<Mutex<Option<AssemblyState>>>,
}

impl<S: Store + Resolver, G: Signer<Key = PublicKey>> PulseAssembler<S, G> {
  pub fn new(signer: G, strand: Strand, store: S) -> Self {
    let period = strand
      .extract_details::<RngStrandDetails>()
      .expect("strand details")
      .period;
    Self {
      builder: TwineBuilder::new(signer),
      strand,
      store,
      rng_path: "./randomness".to_string(),
      state: Arc::new(Mutex::new(None)),
      period,
    }
  }

  pub fn with_rng_path(mut self, rng_path: String) -> Self {
    self.rng_path = rng_path;
    self
  }

  pub async fn init<'a>(&'a self) -> Result<&'a Self> {
    self.load_state().await?;
    Ok(self)
  }

  async fn set_state(&self, state: AssemblyState) {
    *self.state.lock().await = Some(state);
  }

  async fn state(&self) -> AssemblyState {
    self
      .state
      .lock()
      .await
      .clone()
      .expect("state must be loaded by calling init()")
  }

  async fn load_state(&self) -> Result<()> {
    if !{ matches!(*self.state.lock().await, None) } {
      return Ok(());
    }

    let latest = self.latest().await?;
    // if there is no latest, we are starting from scratch
    if latest.is_none() {
      self
        .set_state(AssemblyState::new_from_scratch(self.period))
        .await;
      return Ok(());
    }

    let latest = latest.expect("latest");
    let rng = self.load_rng()?;
    let state = AssemblyState::new_from_latest(latest, rng);
    self.set_state(state.clone()).await;
    Ok(())
  }

  pub async fn needs_assembly(&self) -> bool {
    match self
      .state
      .lock()
      .await
      .as_ref()
      .expect("state must be loaded by calling init()")
    {
      AssemblyState::BeginStrand(_) => true,
      AssemblyState::Prepared { .. } => false,
      AssemblyState::Released { .. } => true,
    }
  }

  pub async fn needs_publish(&self) -> bool {
    match self
      .state
      .lock()
      .await
      .as_ref()
      .expect("state must be loaded by calling init()")
    {
      AssemblyState::BeginStrand(_) => false,
      AssemblyState::Prepared { .. } => true,
      AssemblyState::Released { .. } => false,
    }
  }

  fn rng_file(&self) -> PathBuf {
    PathBuf::from(&self.rng_path).join("rng.dat")
  }

  fn load_rng(&self) -> Result<[u8; 64]> {
    let rng = std::fs::read(&self.rng_file())?;
    if rng.len() != 64 {
      return Err(anyhow::anyhow!("Invalid RNG length {} bytes", rng.len()));
    }
    Ok(rng.try_into().expect("RNG length"))
  }

  fn save_rng(&self, rng: &[u8; 64]) -> Result<()> {
    std::fs::write(self.rng_file(), rng)?;
    Ok(())
  }

  pub async fn prepared(&self) -> Option<Twine> {
    match self.state().await {
      AssemblyState::Prepared { prepared, .. } => Some(prepared),
      _ => None,
    }
  }

  pub async fn next_state_in(
    &self,
    lead_time: Duration,
  ) -> std::time::Duration {
    self
      .state
      .lock()
      .await
      .as_ref()
      .expect("state must be loaded by calling init()")
      .time_till_state_change(lead_time)
  }

  pub async fn previous_cross_stitches(&self) -> CrossStitches {
    match self.state.lock().await.as_ref().expect("state") {
      AssemblyState::BeginStrand(_) => CrossStitches::default(),
      AssemblyState::Prepared { prepared, .. } => prepared.cross_stitches(),
      AssemblyState::Released { latest, .. } => latest.cross_stitches(),
    }
  }

  async fn latest(&self) -> Result<Option<Twine>> {
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
    &self,
    next_randomness: &[u8; 64],
    cross_stitches: CrossStitches,
  ) -> Result<()> {
    if !self.needs_assembly().await {
      return Err(anyhow::anyhow!("Called prepare when it wasn't needed"));
    }

    let next = match self.state().await {
      AssemblyState::BeginStrand(_) => {
        // start the strand
        self.store.save(self.strand.clone()).await?;
        let pb = PayloadBuilder::new(vec![0; 64], next_randomness.to_vec());
        self
          .builder
          .build_first(self.strand.clone())
          .cross_stitches(cross_stitches)
          .build_payload_then_done(pb.builder())?
      }
      AssemblyState::Released { latest, rand } => {
        let pb = PayloadBuilder::new(rand.to_vec(), next_randomness.to_vec());
        self
          .builder
          .build_next(&latest)
          .cross_stitches(cross_stitches)
          .build_payload_then_done(pb.builder())?
      }
      _ => unreachable!(),
    };

    self
      .set_state(AssemblyState::Prepared {
        rand: *next_randomness,
        prepared: next,
      })
      .await;

    Ok(())
  }

  pub async fn publish(&self) -> Result<Twine> {
    if let AssemblyState::Prepared { prepared, rand } = self.state().await {
      self.store.save(prepared.clone()).await?;
      self.save_rng(&rand)?;
      self
        .set_state(AssemblyState::Released {
          latest: prepared.clone(),
          rand,
        })
        .await;
      Ok(prepared)
    } else {
      Err(anyhow::anyhow!("Called publish when not prepared"))
    }
  }
}
