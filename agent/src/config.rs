#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub endpoint: String,
    pub token: String,
    pub interval_secs: u64,
    pub top_n: usize,
    pub disk_scan_interval_secs: u64,
    pub disk_scan_depth: usize,
    pub disk_scan_roots: Vec<String>,
    pub hostname: String,
}

impl Config {
    /// Build a config from a lookup function (testable without touching the real environment).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        let endpoint = lookup("ENDPOINT").unwrap_or_else(|| "http://127.0.0.1:8099/ingest".into());
        let token = lookup("TOKEN").ok_or_else(|| "TOKEN is required".to_string())?;
        let interval_secs = parse_or(&lookup, "INTERVAL_SECS", 60)?;
        let top_n = parse_or(&lookup, "TOP_N", 10)?;
        let disk_scan_interval_secs = parse_or(&lookup, "DISK_SCAN_INTERVAL_SECS", 43_200)?;
        let disk_scan_depth = parse_or(&lookup, "DISK_SCAN_DEPTH", 2)?;
        let disk_scan_roots = lookup("DISK_SCAN_ROOTS")
            .map(|s| s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
            .unwrap_or_else(|| vec!["/".to_string()]);
        let hostname = lookup("HOSTNAME")
            .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string());

        Ok(Config {
            endpoint,
            token,
            interval_secs,
            top_n,
            disk_scan_interval_secs,
            disk_scan_depth,
            disk_scan_roots,
            hostname,
        })
    }

    pub fn from_env() -> Result<Config, String> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }
}

fn parse_or<T: std::str::FromStr>(
    lookup: &impl Fn(&str) -> Option<String>,
    key: &str,
    default: T,
) -> Result<T, String> {
    match lookup(key) {
        None => Ok(default),
        Some(v) => v.parse::<T>().map_err(|_| format!("{key} has invalid value: {v}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn requires_token() {
        let err = Config::from_lookup(lookup_from(&[])).unwrap_err();
        assert!(err.contains("TOKEN"));
    }

    #[test]
    fn applies_defaults() {
        let cfg = Config::from_lookup(lookup_from(&[("TOKEN", "abc")])).unwrap();
        assert_eq!(cfg.endpoint, "http://127.0.0.1:8099/ingest");
        assert_eq!(cfg.interval_secs, 60);
        assert_eq!(cfg.top_n, 10);
        assert_eq!(cfg.disk_scan_interval_secs, 43_200);
        assert_eq!(cfg.disk_scan_depth, 2);
        assert_eq!(cfg.disk_scan_roots, vec!["/".to_string()]);
    }

    #[test]
    fn overrides_all_values() {
        let cfg = Config::from_lookup(lookup_from(&[
            ("TOKEN", "abc"),
            ("ENDPOINT", "https://status.example.com/ingest"),
            ("INTERVAL_SECS", "30"),
            ("TOP_N", "5"),
            ("DISK_SCAN_INTERVAL_SECS", "3600"),
            ("DISK_SCAN_DEPTH", "3"),
            ("DISK_SCAN_ROOTS", "/var, /home"),
            ("HOSTNAME", "custom"),
        ]))
        .unwrap();
        assert_eq!(cfg.endpoint, "https://status.example.com/ingest");
        assert_eq!(cfg.interval_secs, 30);
        assert_eq!(cfg.top_n, 5);
        assert_eq!(cfg.disk_scan_interval_secs, 3600);
        assert_eq!(cfg.disk_scan_depth, 3);
        assert_eq!(cfg.disk_scan_roots, vec!["/var".to_string(), "/home".to_string()]);
        assert_eq!(cfg.hostname, "custom");
    }

    #[test]
    fn rejects_invalid_integer() {
        let err = Config::from_lookup(lookup_from(&[("TOKEN", "abc"), ("TOP_N", "lots")])).unwrap_err();
        assert!(err.contains("TOP_N"));
    }
}
