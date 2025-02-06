use chrono::{DateTime, TimeDelta, Utc};

pub fn next_truncated_time(period: TimeDelta) -> DateTime<Utc> {
  let now = Utc::now();
  use chrono::DurationRound;
  now.duration_trunc(period).unwrap() + period
}

pub fn next_pulse_timestamp(
  prev_time: DateTime<Utc>,
  period: TimeDelta,
) -> DateTime<Utc> {
  let now = Utc::now();
  if now - prev_time < period {
    prev_time + period
  } else {
    next_truncated_time(period)
  }
}

#[cfg(test)]
mod test {
  use chrono::DurationRound;

  use super::*;

  #[test]
  fn test_next_pulse_time() {
    let period = TimeDelta::seconds(60);
    let ts = Utc::now().duration_trunc(period).unwrap();

    let prev_time = ts;
    let next = next_pulse_timestamp(prev_time, period);
    assert_eq!(next, ts + period);

    let prev_time = ts - period;
    let next = next_pulse_timestamp(prev_time, period);
    assert_eq!(next, ts + period);

    let period = TimeDelta::minutes(5);
    let prev_time = ts;
    let next = next_pulse_timestamp(prev_time, period);
    assert_eq!(next, ts + period);
  }
}
