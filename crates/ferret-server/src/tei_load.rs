//! How busy each TEI is, from samples of its metrics: rates need two samples, so the server keeps
//! the recent ones per URL. Read on demand (the Admin page), at most every few seconds.

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::tei::{self, Metrics};

/// Rates are averaged over about this long.
const WINDOW: Duration = Duration::from_secs(60);
/// A newer sample than this is reused instead of asking TEI again.
const FRESH: Duration = Duration::from_secs(3);
/// Two samples closer than this give no meaningful rate.
const MIN_SPAN: Duration = Duration::from_secs(2);
const SCRAPE_TIMEOUT: Duration = Duration::from_secs(3);

/// A TEI's figures. Rates are `None` until two samples far enough apart exist.
pub struct Load {
    pub queue_size: f64,
    pub failures: f64,
    /// Inputs per inference batch.
    pub batch_size: Option<f64>,
    pub throughput_per_minute: Option<f64>,
    /// Share of the window spent in inference, a CPU proxy: above 1 with parallel batches.
    pub load: Option<f64>,
}

#[derive(Default)]
pub struct TeiLoad {
    samples: Mutex<HashMap<String, VecDeque<(Instant, Metrics)>>>,
}

impl TeiLoad {
    /// The figures of the TEI at `url`, `None` while its metrics don't answer.
    pub async fn read(&self, http: &reqwest::Client, url: &str) -> Option<Load> {
        let fresh = self.newest(url).filter(|(at, _)| at.elapsed() < FRESH);
        if fresh.is_none() {
            let metrics = tei::metrics(http, url, SCRAPE_TIMEOUT).await?;
            let mut samples = self.samples.lock().expect("no panics while sampling");
            let samples = samples.entry(url.to_owned()).or_default();
            samples.push_back((Instant::now(), metrics));
            // Keeps one sample older than the window, to span all of it.
            while samples.len() > 2 && samples[1].0.elapsed() > WINDOW {
                samples.pop_front();
            }
        }
        let samples = self.samples.lock().expect("no panics while sampling");
        let samples = samples.get(url)?;
        let (newest_at, newest) = *samples.back()?;
        let oldest = samples
            .iter()
            .find(|(at, _)| newest_at.duration_since(*at) >= MIN_SPAN);
        Some(load(
            oldest.map(|(at, metrics)| (newest_at.duration_since(*at), *metrics)),
            newest,
        ))
    }

    fn newest(&self, url: &str) -> Option<(Instant, Metrics)> {
        let samples = self.samples.lock().expect("no panics while sampling");
        samples.get(url)?.back().copied()
    }
}

/// The figures of `newest`, with rates against an earlier sample `span` before it.
fn load(earlier: Option<(Duration, Metrics)>, newest: Metrics) -> Load {
    let rates = earlier.map(|(span, earlier)| {
        let minutes = span.as_secs_f64() / 60.0;
        let batches = newest.batches - earlier.batches;
        let batch_size =
            (batches > 0.0).then(|| (newest.batched_inputs - earlier.batched_inputs) / batches);
        // Every input of a batch records the batch's inference time, so the sum counts each batch
        // as often as it has inputs.
        let busy =
            batch_size.map(|size| (newest.inference_seconds - earlier.inference_seconds) / size);
        (
            batch_size,
            (newest.embedded - earlier.embedded) / minutes,
            busy.map_or(0.0, |busy| busy / span.as_secs_f64()),
        )
    });
    Load {
        queue_size: newest.queue_size,
        failures: newest.failures,
        batch_size: rates.and_then(|(batch_size, _, _)| batch_size),
        throughput_per_minute: rates.map(|(_, throughput, _)| throughput),
        load: rates.map(|(_, _, load)| load),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_come_from_the_difference_and_inference_time_counts_once_per_batch() {
        let earlier = Metrics {
            embedded: 100.0,
            inference_seconds: 1000.0,
            batched_inputs: 100.0,
            batches: 10.0,
            ..Metrics::default()
        };
        // In 30 s: 2 batches of 8 inputs, each 12 s of inference, recorded 8 times.
        let newest = Metrics {
            queue_size: 5.0,
            embedded: 116.0,
            inference_seconds: 1000.0 + 2.0 * 8.0 * 12.0,
            batched_inputs: 116.0,
            batches: 12.0,
            ..Metrics::default()
        };

        let load = load(Some((Duration::from_secs(30), earlier)), newest);

        assert_eq!(load.queue_size, 5.0);
        assert_eq!(load.batch_size, Some(8.0));
        assert_eq!(load.throughput_per_minute, Some(32.0));
        assert_eq!(load.load, Some(24.0 / 30.0));
        assert!(super::load(None, newest).load.is_none());
    }
}
