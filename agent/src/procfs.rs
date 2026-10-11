//! Pure parsers for `/proc` contents and filesystem usage helpers.

use shared::{DiskIo, Mem, Net};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

/// Parse `/proc/stat`. Returns the aggregate times and one entry per logical core.
pub fn parse_cpu_stat(content: &str) -> (CpuTimes, Vec<CpuTimes>) {
    let mut aggregate = CpuTimes { idle: 0, total: 0 };
    let mut cores = Vec::new();
    for line in content.lines() {
        let Some(rest) = line.strip_prefix("cpu") else {
            continue;
        };
        let is_core = rest
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false);
        let mut values: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        if is_core {
            // Core lines are `cpuN ...`; drop the leading core index.
            values.remove(0);
        }
        if values.len() < 4 {
            continue;
        }
        let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
        let total: u64 = values.iter().take(8).sum();
        let times = CpuTimes { idle, total };
        if is_core {
            cores.push(times);
        } else {
            aggregate = times;
        }
    }
    (aggregate, cores)
}

/// Percentage of non-idle CPU time between two samples.
pub fn cpu_pct(prev: &CpuTimes, cur: &CpuTimes) -> f64 {
    let delta_total = cur.total.saturating_sub(prev.total);
    if delta_total == 0 {
        return 0.0;
    }
    let delta_idle = cur.idle.saturating_sub(prev.idle);
    100.0 * delta_total.saturating_sub(delta_idle) as f64 / delta_total as f64
}

/// Parse `/proc/meminfo` (values are in kB; converted to bytes).
pub fn parse_meminfo(content: &str) -> Mem {
    let mut total = 0u64;
    let mut available = 0u64;
    let mut swap_total = 0u64;
    let mut swap_free = 0u64;
    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or("");
        let kb: u64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let bytes = kb * 1024;
        match key {
            "MemTotal:" => total = bytes,
            "MemAvailable:" => available = bytes,
            "SwapTotal:" => swap_total = bytes,
            "SwapFree:" => swap_free = bytes,
            _ => {}
        }
    }
    Mem {
        total,
        used: total.saturating_sub(available),
        available,
        swap_total,
        swap_used: swap_total.saturating_sub(swap_free),
    }
}

/// Parse `/proc/loadavg` into 1/5/15-minute load averages.
pub fn parse_loadavg(content: &str) -> [f64; 3] {
    let vals: Vec<f64> = content
        .split_whitespace()
        .take(3)
        .filter_map(|t| t.parse().ok())
        .collect();
    [
        vals.first().copied().unwrap_or(0.0),
        vals.get(1).copied().unwrap_or(0.0),
        vals.get(2).copied().unwrap_or(0.0),
    ]
}

/// Parse the first field of `/proc/uptime` (seconds, truncated to u64).
pub fn parse_uptime(content: &str) -> u64 {
    content
        .split_whitespace()
        .next()
        .and_then(|t| t.parse::<f64>().ok())
        .unwrap_or(0.0) as u64
}

/// Parse `/proc/net/dev`; sums all interfaces except loopback.
pub fn parse_netdev(content: &str) -> Net {
    let mut rx_bytes = 0u64;
    let mut tx_bytes = 0u64;
    for line in content.lines() {
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        if iface.trim() == "lo" {
            continue;
        }
        let cols: Vec<&str> = rest.split_whitespace().collect();
        if cols.len() < 9 {
            continue;
        }
        rx_bytes += cols[0].parse::<u64>().unwrap_or(0);
        tx_bytes += cols[8].parse::<u64>().unwrap_or(0);
    }
    Net { rx_bytes, tx_bytes }
}

/// Parse `/proc/diskstats`; sums sectors (512 bytes each) across real block devices.
pub fn parse_diskstats(content: &str) -> DiskIo {
    let mut read_bytes = 0u64;
    let mut write_bytes = 0u64;
    for line in content.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 {
            continue;
        }
        let name = cols[2];
        if name.starts_with("loop") || name.starts_with("ram") {
            continue;
        }
        let sectors_read: u64 = cols[5].parse().unwrap_or(0);
        let sectors_written: u64 = cols[9].parse().unwrap_or(0);
        read_bytes += sectors_read * 512;
        write_bytes += sectors_written * 512;
    }
    DiskIo {
        read_bytes,
        write_bytes,
    }
}

/// Parse `/proc/mounts`; keeps mount points whose source is a real block device.
pub fn parse_mounts(content: &str) -> Vec<String> {
    let mut mounts = Vec::new();
    for line in content.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 2 {
            continue;
        }
        if cols[0].starts_with("/dev/") {
            mounts.push(cols[1].to_string());
        }
    }
    mounts
}

/// Parse a single `/proc/<pid>/stat` into (name, total jiffies).
pub fn parse_proc_stat(content: &str) -> Option<(String, u64)> {
    let open = content.find('(')?;
    let close = content.rfind(')')?;
    let name = content[open + 1..close].to_string();
    let fields: Vec<&str> = content[close + 1..].split_whitespace().collect();
    if fields.len() < 13 {
        return None;
    }
    let utime: u64 = fields[11].parse().ok()?;
    let stime: u64 = fields[12].parse().ok()?;
    Some((name, utime + stime))
}

/// Parse `VmRSS` from `/proc/<pid>/status` into bytes.
pub fn parse_status_rss(content: &str) -> u64 {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            return kb * 1024;
        }
    }
    0
}

/// Total and used bytes for the filesystem containing `path`, via `statvfs`.
pub fn disk_usage(path: &str) -> Option<(u64, u64)> {
    use std::ffi::CString;
    let c_path = CString::new(path).ok()?;
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stat) != 0 {
            return None;
        }
        let total = stat.f_blocks as u64 * stat.f_frsize as u64;
        let free = stat.f_bavail as u64 * stat.f_frsize as u64;
        Some((total, total.saturating_sub(free)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "\
cpu  100 0 50 1000 20 0 5 0 0 0
cpu0 60 0 30 500 10 0 2 0 0 0
cpu1 40 0 20 500 10 0 3 0 0 0
intr 12345 0 0
ctxt 999
";

    const MEMINFO: &str = "\
MemTotal:       16384000 kB
MemFree:         1000000 kB
MemAvailable:    8192000 kB
SwapTotal:       2097152 kB
SwapFree:        1048576 kB
";

    #[test]
    fn parses_cpu_aggregate_and_cores() {
        let (agg, cores) = parse_cpu_stat(STAT);
        assert_eq!(agg.total, 100 + 50 + 1000 + 20 + 5);
        assert_eq!(agg.idle, 1000 + 20);
        assert_eq!(cores.len(), 2);
        assert_eq!(cores[0].total, 60 + 30 + 500 + 10 + 2);
        assert_eq!(cores[0].idle, 500 + 10);
    }

    #[test]
    fn computes_cpu_percentage() {
        let prev = CpuTimes {
            idle: 100,
            total: 200,
        };
        let cur = CpuTimes {
            idle: 120,
            total: 300,
        };
        // 100 total ticks elapsed, 20 idle => 80% busy
        assert!((cpu_pct(&prev, &cur) - 80.0).abs() < 1e-9);
        assert_eq!(cpu_pct(&prev, &prev), 0.0);
    }

    #[test]
    fn parses_meminfo_to_bytes() {
        let mem = parse_meminfo(MEMINFO);
        assert_eq!(mem.total, 16_384_000 * 1024);
        assert_eq!(mem.available, 8_192_000 * 1024);
        assert_eq!(mem.used, 8_192_000 * 1024);
        assert_eq!(mem.swap_total, 2_097_152 * 1024);
        assert_eq!(mem.swap_used, 1_048_576 * 1024);
    }

    #[test]
    fn parses_loadavg_and_uptime() {
        assert_eq!(
            parse_loadavg("0.42 0.55 0.61 1/234 5678"),
            [0.42, 0.55, 0.61]
        );
        assert_eq!(parse_uptime("812345.67 1234567.89"), 812_345);
    }

    const NETDEV: &str = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0
  eth0: 5000 50 0 0 0 0 0 0 8000 80 0 0 0 0 0 0
";

    const DISKSTATS: &str = "\
   8       0 sda 100 0 2000 30 50 0 4000 40 0 0 0 0 0 0
   8       1 sda1 90 0 1900 29 40 0 3900 39 0 0 0 0 0 0
   7       0 loop0 1 0 8 0 0 0 0 0 0 0 0 0 0 0
";

    const MOUNTS: &str = "\
/dev/sda1 / ext4 rw,relatime 0 0
proc /proc proc rw 0 0
tmpfs /run tmpfs rw 0 0
/dev/sdb1 /data ext4 rw 0 0
";

    const PROC_STAT: &str = "1234 (postgres) S 1 1234 1234 0 -1 4194560 1000 0 0 0 15 25 0 0 20 0 12 0 12345 123456789 500 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n";

    #[test]
    fn parses_netdev_excluding_loopback() {
        let net = parse_netdev(NETDEV);
        assert_eq!(net.rx_bytes, 5000);
        assert_eq!(net.tx_bytes, 8000);
    }

    #[test]
    fn parses_diskstats_excluding_loop() {
        let io = parse_diskstats(DISKSTATS);
        assert_eq!(io.read_bytes, (2000 + 1900) * 512);
        assert_eq!(io.write_bytes, (4000 + 3900) * 512);
    }

    #[test]
    fn parses_only_block_device_mounts() {
        assert_eq!(
            parse_mounts(MOUNTS),
            vec!["/".to_string(), "/data".to_string()]
        );
    }

    #[test]
    fn parses_proc_stat_name_and_jiffies() {
        let (name, jiffies) = parse_proc_stat(PROC_STAT).unwrap();
        assert_eq!(name, "postgres");
        assert_eq!(jiffies, 40);
    }

    #[test]
    fn parses_status_rss() {
        let status = "Name:\tpostgres\nVmRSS:\t   204800 kB\n";
        assert_eq!(parse_status_rss(status), 204_800 * 1024);
        assert_eq!(parse_status_rss("Name:\tx\n"), 0);
    }

    #[test]
    fn reads_root_filesystem_usage() {
        let (total, used) = disk_usage("/").unwrap();
        assert!(total > 0);
        assert!(used <= total);
    }
}
