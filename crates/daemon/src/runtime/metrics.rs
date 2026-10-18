use std::sync::Arc;
use std::time::Duration;

use proto::process::{ProcessMetrics, ProcessMetricsEvent, ProcessState};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use super::Runtime;

const TICK: Duration = Duration::from_secs(2);

pub fn spawn_metrics_sampler(runtime: Arc<Runtime>) {
    tokio::spawn(async move {
        let mut system = System::new();
        // `cpu_usage()` sums across cores, so a multi-threaded JVM reports well
        // over 100%; normalize by the logical core count to a 0-100% share of
        // total machine capacity.
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1) as f32;
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            sample(&runtime, &mut system, cores);
        }
    });
}

fn sample(runtime: &Runtime, system: &mut System, cores: f32) {
    if !runtime.hub().has_broadcast_subscriber() {
        return;
    }
    let running: Vec<(String, Pid)> = runtime
        .processes()
        .list()
        .into_iter()
        .filter(|p| p.state == ProcessState::Running)
        .map(|p| (p.id, Pid::from_u32(p.pid)))
        .collect();
    if running.is_empty() {
        return;
    }

    let pids: Vec<Pid> = running.iter().map(|(_, pid)| *pid).collect();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );

    let samples: Vec<ProcessMetrics> = running
        .into_iter()
        .filter_map(|(id, pid)| {
            let process = system.process(pid)?;
            Some(ProcessMetrics {
                id,
                cpu_pct: process.cpu_usage() / cores,
                mem_bytes: process.memory(),
            })
        })
        .collect();
    if samples.is_empty() {
        return;
    }
    runtime
        .hub()
        .publish_topic(&ProcessMetricsEvent { samples });
}
