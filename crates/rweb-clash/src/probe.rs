use crate::error::AppError;
use crate::types::DelayResponse;
use axum::http::StatusCode;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::{watch, Mutex, OwnedSemaphorePermit, Semaphore};

pub const AUTO_PROBE_CONCURRENCY: usize = 8;
pub const MANUAL_PROBE_CONCURRENCY: usize = 4;
pub const GLOBAL_PROBE_CONCURRENCY: usize = 12;
pub const MAX_AUTO_PROBES_PER_SECOND: u64 = 5;
pub const AUTO_PROBE_BATCH_SIZE: i64 = 32;
pub const AUTO_PROBE_TICK_SECONDS: u64 = 5;
pub const MAX_DIRECT_GROUP_PROBES: usize = 50;
pub const MANUAL_GROUP_CONCURRENCY: usize = 4;

const HEALTHY_PROBE_INTERVAL_SECONDS: i64 = 1_800;
const MAX_FAILURE_BACKOFF_SECONDS: i64 = 21_600;

#[derive(Debug, Clone, Copy)]
pub enum ProbeLane {
    Manual,
    Automatic,
}

#[derive(Debug, Clone)]
struct SharedProbeError {
    status: StatusCode,
    code: String,
    message: String,
}

#[derive(Debug, Clone)]
enum SharedProbeOutcome {
    Success(DelayResponse),
    Failure(SharedProbeError),
}

impl SharedProbeOutcome {
    fn from_result(result: &Result<DelayResponse, AppError>) -> Self {
        match result {
            Ok(response) => Self::Success(response.clone()),
            Err(error) => Self::Failure(SharedProbeError {
                status: error.status,
                code: error.code.clone(),
                message: error.message.clone(),
            }),
        }
    }

    fn into_result(self) -> Result<DelayResponse, AppError> {
        match self {
            Self::Success(response) => Ok(response),
            Self::Failure(error) => Err(AppError::new(error.status, error.code, error.message)),
        }
    }
}

#[derive(Debug)]
enum ProbeFlight {
    Leader(watch::Sender<Option<SharedProbeOutcome>>),
    Follower(watch::Receiver<Option<SharedProbeOutcome>>),
}

#[derive(Debug, Clone)]
pub struct ProbeCoordinator {
    global: Arc<Semaphore>,
    automatic: Arc<Semaphore>,
    manual: Arc<Semaphore>,
    in_flight: Arc<Mutex<HashMap<String, watch::Sender<Option<SharedProbeOutcome>>>>>,
    group_in_flight: Arc<Mutex<HashSet<String>>>,
    scheduled: Arc<Mutex<HashSet<String>>>,
    last_dispatch: Arc<Mutex<Instant>>,
}

impl ProbeCoordinator {
    pub fn new() -> Self {
        Self {
            global: Arc::new(Semaphore::new(GLOBAL_PROBE_CONCURRENCY)),
            automatic: Arc::new(Semaphore::new(AUTO_PROBE_CONCURRENCY)),
            manual: Arc::new(Semaphore::new(MANUAL_PROBE_CONCURRENCY)),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            group_in_flight: Arc::new(Mutex::new(HashSet::new())),
            scheduled: Arc::new(Mutex::new(HashSet::new())),
            last_dispatch: Arc::new(Mutex::new(
                Instant::now() - Duration::from_millis(1_000 / MAX_AUTO_PROBES_PER_SECOND),
            )),
        }
    }

    async fn begin(&self, name: &str) -> ProbeFlight {
        let mut in_flight = self.in_flight.lock().await;
        if let Some(sender) = in_flight.get(name) {
            return ProbeFlight::Follower(sender.subscribe());
        }
        let (sender, _) = watch::channel(None);
        in_flight.insert(name.to_string(), sender.clone());
        ProbeFlight::Leader(sender)
    }

    async fn wait_for_flight(
        mut receiver: watch::Receiver<Option<SharedProbeOutcome>>,
    ) -> Result<DelayResponse, AppError> {
        loop {
            if let Some(outcome) = receiver.borrow().clone() {
                return outcome.into_result();
            }
            receiver.changed().await.map_err(|_| {
                AppError::service_unavailable(
                    "probe_unavailable",
                    "the node probe ended before returning a result",
                )
            })?;
        }
    }

    async fn acquire_permits(
        &self,
        lane: ProbeLane,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit), AppError> {
        let global = match lane {
            ProbeLane::Manual => self.global.clone().try_acquire_owned().map_err(|_| {
                AppError::service_unavailable(
                    "probe_busy",
                    "manual probe capacity is currently full",
                )
            })?,
            ProbeLane::Automatic => self.global.clone().acquire_owned().await.map_err(|_| {
                AppError::service_unavailable("probe_unavailable", "probe capacity is closed")
            })?,
        };
        let lane_permit = match lane {
            ProbeLane::Manual => self.manual.clone().try_acquire_owned().map_err(|_| {
                AppError::service_unavailable(
                    "probe_busy",
                    "manual probe capacity is currently full",
                )
            })?,
            ProbeLane::Automatic => self.automatic.clone().acquire_owned().await.map_err(|_| {
                AppError::service_unavailable("probe_unavailable", "probe lane is closed")
            })?,
        };
        Ok((global, lane_permit))
    }

    async fn wait_for_rate_limit(&self) {
        let interval = Duration::from_millis(1_000 / MAX_AUTO_PROBES_PER_SECOND);
        let mut last_dispatch = self.last_dispatch.lock().await;
        let now = Instant::now();
        let next = *last_dispatch + interval;
        if next > now {
            tokio::time::sleep(next - now).await;
        }
        *last_dispatch = Instant::now();
    }

    pub async fn run<F, Fut>(
        &self,
        name: &str,
        lane: ProbeLane,
        operation: F,
    ) -> Result<DelayResponse, AppError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<DelayResponse, AppError>>,
    {
        match self.begin(name).await {
            ProbeFlight::Follower(receiver) => Self::wait_for_flight(receiver).await,
            ProbeFlight::Leader(sender) => {
                let result = async {
                    let _permits = self.acquire_permits(lane).await?;
                    self.wait_for_rate_limit().await;
                    operation().await
                }
                .await;
                let outcome = SharedProbeOutcome::from_result(&result);
                let _ = sender.send(Some(outcome));
                {
                    let mut in_flight = self.in_flight.lock().await;
                    in_flight.remove(name);
                }
                result
            }
        }
    }

    pub async fn run_group<F, Fut>(
        &self,
        name: &str,
        operation: F,
    ) -> Result<Vec<DelayResponse>, AppError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Vec<DelayResponse>, AppError>>,
    {
        if !self.group_in_flight.lock().await.insert(name.to_string()) {
            return Err(AppError::service_unavailable(
                "probe_busy",
                "this proxy group is already being tested",
            ));
        }
        let result = async {
            let _permits = self.acquire_permits(ProbeLane::Manual).await?;
            self.wait_for_rate_limit().await;
            operation().await
        }
        .await;
        self.group_in_flight.lock().await.remove(name);
        result
    }

    pub async fn try_claim_automatic(&self, name: &str) -> bool {
        self.scheduled.lock().await.insert(name.to_string())
    }

    pub async fn release_automatic(&self, name: &str) {
        self.scheduled.lock().await.remove(name);
    }
}

impl Default for ProbeCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

pub fn next_probe_at(name: &str, success: bool, consecutive_failures: u32) -> String {
    let seconds = probe_delay_seconds(name, success, consecutive_failures);
    (OffsetDateTime::now_utc() + time::Duration::seconds(seconds))
        .format(&Rfc3339)
        .unwrap_or_else(|_| "9999-12-31T23:59:59Z".into())
}

fn probe_delay_seconds(name: &str, success: bool, consecutive_failures: u32) -> i64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    let hash = hasher.finish();
    let jitter_limit = if success { 300 } else { 60 };
    let jitter = (hash % (jitter_limit + 1)) as i64;
    let base = if success {
        HEALTHY_PROBE_INTERVAL_SECONDS
    } else {
        let exponent = consecutive_failures.saturating_sub(1).min(9);
        (60_i64.saturating_mul(1_i64 << exponent)).min(MAX_FAILURE_BACKOFF_SECONDS)
    };
    base.saturating_add(jitter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn failure_backoff_grows_and_is_capped() {
        let first = probe_delay_seconds("node", false, 1);
        let fifth = probe_delay_seconds("node", false, 5);
        let capped = probe_delay_seconds("node", false, 32);
        assert!((60..=120).contains(&first));
        assert!((960..=1_020).contains(&fifth));
        assert!((MAX_FAILURE_BACKOFF_SECONDS..=MAX_FAILURE_BACKOFF_SECONDS + 60).contains(&capped));
    }

    #[tokio::test]
    async fn concurrent_node_probes_share_one_operation() {
        let coordinator = ProbeCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let first_calls = calls.clone();
        let second_calls = calls.clone();
        let first_coordinator = coordinator.clone();
        let first = first_coordinator.run("node", ProbeLane::Manual, move || async move {
            first_calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok(DelayResponse {
                name: "node".into(),
                delay: 42,
            })
        });
        let second = coordinator.run("node", ProbeLane::Manual, move || async move {
            second_calls.fetch_add(1, Ordering::SeqCst);
            Ok(DelayResponse {
                name: "node".into(),
                delay: 43,
            })
        });
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.unwrap().delay, 42);
        assert_eq!(second.unwrap().delay, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
