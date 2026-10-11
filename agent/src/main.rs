use std::thread::sleep;
use std::time::{Duration, Instant};

use server_status_agent::collect::Collector;
use server_status_agent::config::Config;
use server_status_agent::{diskscan, push};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let once = args.iter().any(|a| a == "--once");
    let dry_run = args.iter().any(|a| a == "--dry-run");

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("config error: {err}");
            std::process::exit(2);
        }
    };

    let mut collector = Collector::new();
    let mut disk_space = diskscan::scan(
        &config.disk_scan_roots,
        config.disk_scan_depth,
        config.top_n,
    );
    let mut next_scan = Instant::now() + Duration::from_secs(config.disk_scan_interval_secs);

    // Establish a baseline so the first CPU/process reading is meaningful.
    sleep(Duration::from_secs(1));

    loop {
        if Instant::now() >= next_scan {
            disk_space = diskscan::scan(
                &config.disk_scan_roots,
                config.disk_scan_depth,
                config.top_n,
            );
            next_scan = Instant::now() + Duration::from_secs(config.disk_scan_interval_secs);
        }

        let snapshot = collector.collect(&config, disk_space.clone());

        if dry_run {
            match serde_json::to_string_pretty(&snapshot) {
                Ok(json) => println!("{json}"),
                Err(err) => eprintln!("serialize error: {err}"),
            }
        } else {
            match push::push(&config.endpoint, &config.token, &snapshot) {
                Ok(()) => eprintln!("pushed ts={}", snapshot.ts),
                Err(err) => eprintln!("push failed: {err}"),
            }
        }

        if once || dry_run {
            break;
        }
        sleep(Duration::from_secs(config.interval_secs));
    }
}
