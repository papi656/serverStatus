//! Process enumeration and top-N ranking.

use std::collections::HashMap;

use shared::ProcessEntry;

use crate::procfs::{parse_proc_stat, parse_status_rss};

#[derive(Debug, Clone, PartialEq)]
pub struct RawProc {
    pub pid: i32,
    pub name: String,
    pub jiffies: u64,
    pub rss_bytes: u64,
}

/// Read every process under `proc_root` (usually `/proc`).
pub fn read_processes(proc_root: &str) -> Vec<RawProc> {
    let mut processes = Vec::new();
    let Ok(dir) = std::fs::read_dir(proc_root) else { return processes };
    for entry in dir.flatten() {
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
        let base = entry.path();
        let Ok(stat) = std::fs::read_to_string(base.join("stat")) else { continue };
        let Some((name, jiffies)) = parse_proc_stat(&stat) else { continue };
        let rss_bytes = std::fs::read_to_string(base.join("status"))
            .map(|s| parse_status_rss(&s))
            .unwrap_or(0);
        processes.push(RawProc { pid, name, jiffies, rss_bytes });
    }
    processes
}

/// Turn raw process readings into entries with CPU% and memory%.
///
/// CPU% is the process's jiffy delta over the machine's total jiffy delta, scaled to a
/// single core (`* ncpu * 100`). Processes with no previous reading report 0%.
pub fn compute_entries(
    raws: &[RawProc],
    prev: &HashMap<i32, u64>,
    total_delta: u64,
    ncpu: usize,
    mem_total: u64,
) -> Vec<ProcessEntry> {
    raws.iter()
        .map(|p| {
            let cpu_pct = match prev.get(&p.pid) {
                Some(prev_j) if total_delta > 0 => {
                    let delta = p.jiffies.saturating_sub(*prev_j);
                    delta as f64 / total_delta as f64 * ncpu as f64 * 100.0
                }
                _ => 0.0,
            };
            let mem_pct = if mem_total > 0 {
                p.rss_bytes as f64 / mem_total as f64 * 100.0
            } else {
                0.0
            };
            ProcessEntry { pid: p.pid, name: p.name.clone(), cpu_pct, mem_pct }
        })
        .collect()
}

pub fn top_by_cpu(entries: &[ProcessEntry], n: usize) -> Vec<ProcessEntry> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| {
        b.cpu_pct
            .partial_cmp(&a.cpu_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.pid.cmp(&b.pid))
    });
    sorted.truncate(n);
    sorted
}

pub fn top_by_mem(entries: &[ProcessEntry], n: usize) -> Vec<ProcessEntry> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| {
        b.mem_pct
            .partial_cmp(&a.mem_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.pid.cmp(&b.pid))
    });
    sorted.truncate(n);
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(pid: i32, jiffies: u64, rss_bytes: u64) -> RawProc {
        RawProc { pid, name: format!("p{pid}"), jiffies, rss_bytes }
    }

    #[test]
    fn computes_cpu_and_mem_percentages() {
        let prev = HashMap::from([(1, 0u64), (2, 0u64)]);
        let raws = vec![raw(1, 50, 100), raw(2, 10, 900), raw(3, 0, 0)];
        let entries = compute_entries(&raws, &prev, 100, 2, 1000);

        let one = entries.iter().find(|e| e.pid == 1).unwrap();
        assert!((one.cpu_pct - 100.0).abs() < 1e-9); // 50/100 * 2 * 100
        let two = entries.iter().find(|e| e.pid == 2).unwrap();
        assert!((two.mem_pct - 90.0).abs() < 1e-9);
        let three = entries.iter().find(|e| e.pid == 3).unwrap();
        assert_eq!(three.cpu_pct, 0.0); // new process, no baseline
    }

    #[test]
    fn ranks_top_cpu_and_mem() {
        let prev = HashMap::from([(1, 0u64), (2, 0u64)]);
        let raws = vec![raw(1, 50, 100), raw(2, 10, 900)];
        let entries = compute_entries(&raws, &prev, 100, 2, 1000);
        assert_eq!(top_by_cpu(&entries, 1)[0].pid, 1);
        assert_eq!(top_by_mem(&entries, 1)[0].pid, 2);
    }

    #[test]
    fn truncates_to_n() {
        let entries = vec![
            ProcessEntry { pid: 1, name: "a".into(), cpu_pct: 3.0, mem_pct: 1.0 },
            ProcessEntry { pid: 2, name: "b".into(), cpu_pct: 2.0, mem_pct: 2.0 },
            ProcessEntry { pid: 3, name: "c".into(), cpu_pct: 1.0, mem_pct: 3.0 },
        ];
        assert_eq!(top_by_cpu(&entries, 2).len(), 2);
        assert_eq!(top_by_cpu(&entries, 2)[1].pid, 2);
    }
}
