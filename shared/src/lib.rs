//! Wire contract shared between the agent and the dashboard.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub ts: u64,
    pub hostname: String,
    pub uptime_s: u64,
    pub load: [f64; 3],
    pub cpu: Cpu,
    pub mem: Mem,
    pub disks: Vec<DiskUsage>,
    pub net: Net,
    pub diskio: DiskIo,
    pub disk_space: DiskSpace,
    pub processes: Processes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cpu {
    pub total_pct: f64,
    pub per_core_pct: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mem {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskUsage {
    pub mount: String,
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Net {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskIo {
    pub read_bytes: u64,
    pub write_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskSpace {
    pub scanned_at: u64,
    pub roots: Vec<String>,
    pub top_dirs: Vec<DiskItem>,
    pub top_files: Vec<DiskItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskItem {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Processes {
    pub count: usize,
    pub top_cpu: Vec<ProcessEntry>,
    pub top_mem: Vec<ProcessEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessEntry {
    pub pid: i32,
    pub name: String,
    pub cpu_pct: f64,
    pub mem_pct: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Snapshot {
        Snapshot {
            ts: 1_730_000_000,
            hostname: "vps-1".into(),
            uptime_s: 812_345,
            load: [0.42, 0.55, 0.61],
            cpu: Cpu { total_pct: 12.3, per_core_pct: vec![10.1, 15.0] },
            mem: Mem {
                total: 8_589_934_592,
                used: 3_221_225_472,
                available: 5_368_709_120,
                swap_total: 0,
                swap_used: 0,
            },
            disks: vec![DiskUsage { mount: "/".into(), total: 53_687_091_200, used: 21_474_836_480 }],
            net: Net { rx_bytes: 123_456_789, tx_bytes: 987_654_321 },
            diskio: DiskIo { read_bytes: 55_555_555, write_bytes: 44_444_444 },
            disk_space: DiskSpace {
                scanned_at: 1_729_996_000,
                roots: vec!["/".into()],
                top_dirs: vec![DiskItem { path: "/var/lib".into(), bytes: 21_474_836_480 }],
                top_files: vec![DiskItem { path: "/var/log/syslog".into(), bytes: 5_368_709_120 }],
            },
            processes: Processes {
                count: 132,
                top_cpu: vec![ProcessEntry { pid: 812, name: "postgres".into(), cpu_pct: 4.1, mem_pct: 22.4 }],
                top_mem: vec![ProcessEntry { pid: 812, name: "postgres".into(), cpu_pct: 4.1, mem_pct: 22.4 }],
            },
        }
    }

    #[test]
    fn round_trips_through_json() {
        let original = sample();
        let json = serde_json::to_string(&original).unwrap();
        let decoded: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn serializes_expected_top_level_keys() {
        let json = serde_json::to_string(&sample()).unwrap();
        for key in [
            "\"ts\"", "\"hostname\"", "\"uptime_s\"", "\"load\"", "\"cpu\"", "\"mem\"",
            "\"disks\"", "\"net\"", "\"diskio\"", "\"disk_space\"", "\"processes\"",
        ] {
            assert!(json.contains(key), "missing key {key}");
        }
    }
}
