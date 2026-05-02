//! V2 Phase 6B same-host local worker contract/runtime.
//!
//! Local-only proof model: no network listener, no public API, no credentials,
//! no real destination publishing, and no FFmpeg topology changes. The runtime
//! simulates a same-host worker process boundary for one non-customer test
//! output and records route/fencing decisions as proof logs.

use std::collections::HashMap;
use std::time::Duration;

use super::gpu_worker_shadow::{JobLease, RouteGeneration, WorkerId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalRoute {
    CurrentInProcessFfmpeg,
    SameHostWorker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalWorkerPolicy {
    pub gpu_workers_enabled: bool,
    pub is_test_output: bool,
    pub destination_is_local_fake_sink: bool,
}

impl LocalWorkerPolicy {
    pub fn select_route(&self) -> LocalRoute {
        if self.gpu_workers_enabled && self.is_test_output && self.destination_is_local_fake_sink {
            LocalRoute::SameHostWorker
        } else {
            LocalRoute::CurrentInProcessFfmpeg
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalJobState {
    Assigned,
    Activated,
    Healthy,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalJobHealth {
    pub lease_id: String,
    pub output_id: String,
    pub worker_id: WorkerId,
    pub route_generation: RouteGeneration,
    pub state: LocalJobState,
}

#[derive(Debug)]
pub struct LocalWorkerRuntime {
    worker_id: WorkerId,
    running: bool,
    current: HashMap<String, JobLease>,
    health_by_output: HashMap<String, LocalJobHealth>,
    logs: Vec<String>,
}

impl LocalWorkerRuntime {
    pub fn new(worker_id: impl Into<String>) -> Self {
        Self {
            worker_id: WorkerId(worker_id.into()),
            running: false,
            current: HashMap::new(),
            health_by_output: HashMap::new(),
            logs: Vec::new(),
        }
    }

    pub fn worker_id(&self) -> &WorkerId {
        &self.worker_id
    }

    pub fn start(&mut self) {
        if self.running {
            return;
        }
        self.running = true;
        self.logs.push(format!(
            "gpu.worker.local.started worker_id={} transport=stdio local_only=true network_listener=false",
            self.worker_id.0
        ));
        self.logs.push(format!(
            "gpu.worker.local.ready worker_id={} max_outputs=1 destination=local_fake_sink",
            self.worker_id.0
        ));
    }

    pub fn assign(&mut self, lease: JobLease) -> Result<(), &'static str> {
        if !self.running {
            return Err("local worker not running");
        }
        if lease.worker_id != self.worker_id {
            return Err("lease worker mismatch");
        }
        self.logs.push(format!(
            "gpu.job.local.assigned lease_id={} worker_id={} route_generation={} output_id={} route_owner=local_worker",
            lease.lease_id, lease.worker_id.0, lease.route_generation.0, lease.output_id
        ));
        self.current.insert(lease.output_id.clone(), lease);
        Ok(())
    }

    pub fn activate(&mut self, output_id: &str) -> Result<(), &'static str> {
        let lease = self.current.get(output_id).ok_or("lease missing")?;
        self.health_by_output.insert(
            output_id.to_string(),
            LocalJobHealth {
                lease_id: lease.lease_id.clone(),
                output_id: lease.output_id.clone(),
                worker_id: lease.worker_id.clone(),
                route_generation: lease.route_generation,
                state: LocalJobState::Activated,
            },
        );
        self.logs.push(format!(
            "gpu.job.local.activated lease_id={} worker_id={} route_generation={} output_id={}",
            lease.lease_id, lease.worker_id.0, lease.route_generation.0, lease.output_id
        ));
        Ok(())
    }

    pub fn accept_health(&mut self, health: LocalJobHealth) -> bool {
        let Some(current) = self.current.get(&health.output_id) else {
            return false;
        };
        if current.lease_id != health.lease_id
            || current.worker_id != health.worker_id
            || current.route_generation != health.route_generation
        {
            return false;
        }
        self.logs.push(format!(
            "gpu.job.local.health lease_id={} worker_id={} route_generation={} output_id={} state={:?}",
            health.lease_id, health.worker_id.0, health.route_generation.0, health.output_id, health.state
        ));
        self.health_by_output
            .insert(health.output_id.clone(), health);
        true
    }

    pub fn revoke_stale_worker(
        &mut self,
        worker_id: &WorkerId,
        stale_after: Duration,
        observed_gap: Duration,
    ) -> Vec<String> {
        if worker_id != &self.worker_id || observed_gap <= stale_after {
            return Vec::new();
        }
        let mut revoked = Vec::new();
        let outputs: Vec<String> = self.current.keys().cloned().collect();
        for output_id in outputs {
            if let Some(lease) = self.current.remove(&output_id) {
                self.logs.push(format!(
                    "gpu.job.local.revoked lease_id={} worker_id={} route_generation={} output_id={} reason=stale_worker fallback=current_in_process_ffmpeg",
                    lease.lease_id, lease.worker_id.0, lease.route_generation.0, lease.output_id
                ));
                revoked.push(output_id);
            }
        }
        revoked.sort();
        revoked
    }

    pub fn stop(&mut self) {
        if !self.running {
            return;
        }
        self.running = false;
        self.logs.push(format!(
            "gpu.worker.local.stopped worker_id={} fallback=current_in_process_ffmpeg",
            self.worker_id.0
        ));
    }

    pub fn logs(&self) -> &[String] {
        &self.logs
    }
}

pub fn run_one_local_test_output_proof(
    enabled: bool,
    fail_worker: bool,
) -> (LocalRoute, Vec<String>) {
    let policy = LocalWorkerPolicy {
        gpu_workers_enabled: enabled,
        is_test_output: true,
        destination_is_local_fake_sink: true,
    };
    if policy.select_route() != LocalRoute::SameHostWorker {
        return (LocalRoute::CurrentInProcessFfmpeg, Vec::new());
    }

    let mut runtime = LocalWorkerRuntime::new("local-worker-6b-0");
    runtime.start();
    let lease = JobLease::new(
        "local-test-session",
        "local-test-output",
        runtime.worker_id().clone(),
        RouteGeneration(1),
    );
    let output_id = lease.output_id.clone();
    runtime.assign(lease.clone()).expect("local worker started");
    runtime
        .activate(&output_id)
        .expect("assigned lease activates");
    let accepted = runtime.accept_health(LocalJobHealth {
        lease_id: lease.lease_id,
        output_id,
        worker_id: lease.worker_id,
        route_generation: lease.route_generation,
        state: LocalJobState::Healthy,
    });
    assert!(accepted, "current local lease health must pass fencing");
    if fail_worker {
        let worker_id = runtime.worker_id().clone();
        runtime.revoke_stale_worker(&worker_id, Duration::from_secs(2), Duration::from_secs(3));
        runtime.stop();
        return (LocalRoute::CurrentInProcessFfmpeg, runtime.logs().to_vec());
    }
    runtime.stop();
    (LocalRoute::SameHostWorker, runtime.logs().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_off_keeps_current_route_and_emits_no_local_worker_logs() {
        let (route, logs) = run_one_local_test_output_proof(false, false);
        assert_eq!(route, LocalRoute::CurrentInProcessFfmpeg);
        assert!(logs.is_empty());
    }

    #[test]
    fn route_selection_rejects_customer_or_real_destination_by_default() {
        assert_eq!(
            LocalWorkerPolicy {
                gpu_workers_enabled: true,
                is_test_output: false,
                destination_is_local_fake_sink: true
            }
            .select_route(),
            LocalRoute::CurrentInProcessFfmpeg
        );
        assert_eq!(
            LocalWorkerPolicy {
                gpu_workers_enabled: true,
                is_test_output: true,
                destination_is_local_fake_sink: false
            }
            .select_route(),
            LocalRoute::CurrentInProcessFfmpeg
        );
    }

    #[test]
    fn worker_start_ready_stop_lifecycle_logs_local_only() {
        let (route, logs) = run_one_local_test_output_proof(true, false);
        assert_eq!(route, LocalRoute::SameHostWorker);
        for needle in [
            "gpu.worker.local.started",
            "gpu.worker.local.ready",
            "gpu.job.local.assigned",
            "gpu.job.local.activated",
            "gpu.job.local.health",
            "gpu.worker.local.stopped",
        ] {
            assert!(
                logs.iter().any(|line| line.contains(needle)),
                "missing {needle}"
            );
        }
        assert!(
            !logs
                .iter()
                .any(|line| line.contains("rtmp://") || line.contains("rtmps://"))
        );
        assert!(
            logs.iter()
                .any(|line| line.contains("network_listener=false"))
        );
    }

    #[test]
    fn health_is_fenced_by_lease_worker_and_generation() {
        let mut runtime = LocalWorkerRuntime::new("w-current");
        runtime.start();
        let lease = JobLease::new("s", "out", runtime.worker_id().clone(), RouteGeneration(2));
        runtime.assign(lease.clone()).unwrap();
        assert!(!runtime.accept_health(LocalJobHealth {
            lease_id: "wrong-lease".into(),
            output_id: "out".into(),
            worker_id: lease.worker_id.clone(),
            route_generation: lease.route_generation,
            state: LocalJobState::Healthy,
        }));
        assert!(!runtime.accept_health(LocalJobHealth {
            lease_id: lease.lease_id.clone(),
            output_id: "out".into(),
            worker_id: WorkerId("wrong-worker".into()),
            route_generation: lease.route_generation,
            state: LocalJobState::Healthy,
        }));
        assert!(!runtime.accept_health(LocalJobHealth {
            lease_id: lease.lease_id.clone(),
            output_id: "out".into(),
            worker_id: lease.worker_id.clone(),
            route_generation: RouteGeneration(1),
            state: LocalJobState::Healthy,
        }));
        assert!(runtime.accept_health(LocalJobHealth {
            lease_id: lease.lease_id,
            output_id: "out".into(),
            worker_id: lease.worker_id,
            route_generation: lease.route_generation,
            state: LocalJobState::Healthy,
        }));
    }

    #[test]
    fn stale_worker_revoke_falls_back_to_current_route() {
        let (route, logs) = run_one_local_test_output_proof(true, true);
        assert_eq!(route, LocalRoute::CurrentInProcessFfmpeg);
        assert!(
            logs.iter()
                .any(|line| line.contains("gpu.job.local.revoked"))
        );
        assert!(
            logs.iter()
                .any(|line| line.contains("fallback=current_in_process_ffmpeg"))
        );
    }
}
