//! V2 Phase 6A GPU-worker shadow contracts.
//!
//! Pure local model only: no process spawn, media routing, publishing, FFmpeg
//! topology, queues, deploy, or credential handling. The shadow harness emits
//! proof strings when the caller passes `BRIVVA_V2_GPU_WORKERS=true` config.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RouteGeneration(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCapacity {
    pub max_outputs: u16,
    pub assigned_outputs: u16,
    pub nvenc_slots: u16,
    pub shadow_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRegistration {
    pub worker_id: WorkerId,
    pub protocol_version: u16,
    pub capacity: WorkerCapacity,
    pub registered_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerHeartbeat {
    pub worker_id: WorkerId,
    pub route_generation: RouteGeneration,
    pub observed_at_ms: u64,
    pub capacity: WorkerCapacity,
}

impl WorkerHeartbeat {
    pub fn is_stale_at(&self, now_ms: u64, stale_after: Duration) -> bool {
        now_ms.saturating_sub(self.observed_at_ms) > stale_after.as_millis() as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobLease {
    pub lease_id: String,
    pub session_id: String,
    pub output_id: String,
    pub worker_id: WorkerId,
    pub route_generation: RouteGeneration,
}

impl JobLease {
    pub fn new(
        session_id: impl Into<String>,
        output_id: impl Into<String>,
        worker_id: WorkerId,
        route_generation: RouteGeneration,
    ) -> Self {
        let session_id = session_id.into();
        let output_id = output_id.into();
        let lease_id = format!(
            "lease:{}:{}:{}:{}",
            session_id, output_id, route_generation.0, worker_id.0
        );
        Self {
            lease_id,
            session_id,
            output_id,
            worker_id,
            route_generation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobAssign {
    pub command_id: String,
    pub lease: JobLease,
    pub shadow_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Preflighted,
    Healthy,
    Stopped,
    WorkerDead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobHealth {
    pub command_id: String,
    pub lease_id: String,
    pub output_id: String,
    pub worker_id: WorkerId,
    pub route_generation: RouteGeneration,
    pub state: JobState,
}

#[derive(Debug, Default)]
pub struct GpuWorkerShadowModel {
    latest_generation_by_output: HashMap<String, RouteGeneration>,
    health_by_output: HashMap<String, JobHealth>,
    seen_command_ids: HashSet<String>,
}

impl GpuWorkerShadowModel {
    pub fn assign(&mut self, assign: JobAssign) -> bool {
        if !self.seen_command_ids.insert(assign.command_id.clone()) {
            return false;
        }
        self.latest_generation_by_output.insert(
            assign.lease.output_id.clone(),
            assign.lease.route_generation,
        );
        true
    }

    pub fn accept_health(&mut self, health: JobHealth) -> bool {
        if self
            .latest_generation_by_output
            .get(&health.output_id)
            .copied()
            != Some(health.route_generation)
        {
            return false;
        }
        self.health_by_output
            .insert(health.output_id.clone(), health);
        true
    }

    pub fn mark_worker_dead(&mut self, worker_id: &WorkerId) -> Vec<String> {
        let mut affected = Vec::new();
        for (output_id, health) in self.health_by_output.iter_mut() {
            if &health.worker_id == worker_id {
                health.state = JobState::WorkerDead;
                affected.push(output_id.clone());
            }
        }
        affected.sort();
        affected
    }
}

pub fn shadow_proof_logs(enabled: bool, now: SystemTime) -> Vec<String> {
    if !enabled {
        return Vec::new();
    }
    let now_ms = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let worker_id = WorkerId("local-shadow-worker-0".to_string());
    let capacity = WorkerCapacity {
        max_outputs: 1,
        assigned_outputs: 0,
        nvenc_slots: 0,
        shadow_only: true,
    };
    let registration = WorkerRegistration {
        worker_id: worker_id.clone(),
        protocol_version: 1,
        capacity: capacity.clone(),
        registered_at_ms: now_ms,
    };
    let heartbeat = WorkerHeartbeat {
        worker_id: worker_id.clone(),
        route_generation: RouteGeneration(1),
        observed_at_ms: now_ms,
        capacity,
    };
    let lease = JobLease::new(
        "shadow-session",
        "shadow-session:ja:youtube:0",
        worker_id,
        RouteGeneration(1),
    );

    vec![
        format!(
            "gpu.worker.shadow.registered worker_id={} protocol_version={} worker.register shadow",
            registration.worker_id.0, registration.protocol_version
        ),
        format!(
            "gpu.worker.shadow.heartbeat worker_id={} route_generation={} worker.heartbeat shadow",
            heartbeat.worker_id.0, heartbeat.route_generation.0
        ),
        format!(
            "gpu.job.shadow.assigned lease_id={} output_id={} job.assign shadow",
            lease.lease_id, lease.output_id
        ),
        format!(
            "gpu.job.shadow.preflighted lease_id={} job.preflighted shadow",
            lease.lease_id
        ),
        format!(
            "gpu.job.shadow.health lease_id={} state=healthy job.health shadow",
            lease.lease_id
        ),
        format!(
            "gpu.job.shadow.stopped lease_id={} job.stop shadow",
            lease.lease_id
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease(generation: u64, output: &str, worker: &str) -> JobLease {
        JobLease::new(
            "s1",
            output,
            WorkerId(worker.into()),
            RouteGeneration(generation),
        )
    }

    #[test]
    fn lease_id_format_is_stable() {
        let a = lease(7, "s1:ja:youtube:0", "w-a");
        let b = lease(7, "s1:ja:youtube:0", "w-a");
        assert_eq!(a.lease_id, "lease:s1:s1:ja:youtube:0:7:w-a");
        assert_eq!(a.lease_id, b.lease_id);
    }

    #[test]
    fn stale_heartbeat_detection_uses_observed_time() {
        let hb = WorkerHeartbeat {
            worker_id: WorkerId("w-a".into()),
            route_generation: RouteGeneration(1),
            observed_at_ms: 1_000,
            capacity: WorkerCapacity {
                max_outputs: 1,
                assigned_outputs: 0,
                nvenc_slots: 0,
                shadow_only: true,
            },
        };
        assert!(!hb.is_stale_at(1_500, Duration::from_millis(500)));
        assert!(hb.is_stale_at(1_501, Duration::from_millis(500)));
    }

    #[test]
    fn route_generation_fences_stale_worker_health() {
        let mut model = GpuWorkerShadowModel::default();
        let current = lease(2, "out-a", "w-a");
        assert!(model.assign(JobAssign {
            command_id: "cmd-2".into(),
            lease: current.clone(),
            shadow_only: true
        }));
        let stale_health = JobHealth {
            command_id: "health-1".into(),
            lease_id: "lease:old".into(),
            output_id: "out-a".into(),
            worker_id: WorkerId("w-a".into()),
            route_generation: RouteGeneration(1),
            state: JobState::Healthy,
        };
        assert!(!model.accept_health(stale_health));
        assert!(model.accept_health(JobHealth {
            command_id: "health-2".into(),
            lease_id: current.lease_id,
            output_id: "out-a".into(),
            worker_id: WorkerId("w-a".into()),
            route_generation: RouteGeneration(2),
            state: JobState::Healthy,
        }));
    }

    #[test]
    fn duplicate_command_id_is_idempotent() {
        let mut model = GpuWorkerShadowModel::default();
        let first = JobAssign {
            command_id: "cmd-1".into(),
            lease: lease(1, "out-a", "w-a"),
            shadow_only: true,
        };
        let duplicate = JobAssign {
            command_id: "cmd-1".into(),
            lease: lease(2, "out-a", "w-b"),
            shadow_only: true,
        };
        assert!(model.assign(first));
        assert!(!model.assign(duplicate));
    }

    #[test]
    fn worker_dead_affects_only_leased_output_jobs() {
        let mut model = GpuWorkerShadowModel::default();
        for (output, worker) in [("out-a", "w-a"), ("out-b", "w-b"), ("out-c", "w-a")] {
            let l = lease(1, output, worker);
            assert!(model.assign(JobAssign {
                command_id: format!("cmd-{output}"),
                lease: l.clone(),
                shadow_only: true
            }));
            assert!(model.accept_health(JobHealth {
                command_id: format!("health-{output}"),
                lease_id: l.lease_id,
                output_id: output.into(),
                worker_id: WorkerId(worker.into()),
                route_generation: RouteGeneration(1),
                state: JobState::Healthy,
            }));
        }
        assert_eq!(
            model.mark_worker_dead(&WorkerId("w-a".into())),
            vec!["out-a".to_string(), "out-c".to_string()]
        );
        assert_eq!(model.health_by_output["out-b"].state, JobState::Healthy);
    }

    #[test]
    fn flag_off_emits_no_shadow_logs_and_flag_on_emits_proof_only_logs() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        assert!(shadow_proof_logs(false, now).is_empty());
        let logs = shadow_proof_logs(true, now);
        assert_eq!(logs.len(), 6);
        for needle in [
            "gpu.worker.shadow.registered",
            "gpu.worker.shadow.heartbeat",
            "gpu.job.shadow.assigned",
            "gpu.job.shadow.preflighted",
            "gpu.job.shadow.health",
            "gpu.job.shadow.stopped",
        ] {
            assert!(
                logs.iter().any(|line| line.contains(needle)),
                "missing {needle}"
            );
        }
        assert!(
            !logs
                .iter()
                .any(|line| line.contains("rtmp://") || line.contains("ffmpeg"))
        );
    }
}
