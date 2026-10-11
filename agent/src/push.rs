//! Push a snapshot to the dashboard's ingest endpoint.

use std::time::Duration;

use shared::Snapshot;

pub fn push(endpoint: &str, token: &str, snapshot: &Snapshot) -> Result<(), String> {
    let body = serde_json::to_string(snapshot).map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build();
    let response = agent
        .post(endpoint)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&body);
    match response {
        Ok(resp) if resp.status() == 200 || resp.status() == 204 => Ok(()),
        Ok(resp) => Err(format!("unexpected status {}", resp.status())),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::*;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener};
    use std::thread::JoinHandle;

    fn sample() -> Snapshot {
        Snapshot {
            ts: 1,
            hostname: "vps-1".into(),
            uptime_s: 10,
            load: [0.0, 0.0, 0.0],
            cpu: Cpu {
                total_pct: 0.0,
                per_core_pct: vec![0.0],
            },
            mem: Mem {
                total: 1,
                used: 0,
                available: 1,
                swap_total: 0,
                swap_used: 0,
            },
            disks: vec![],
            net: Net {
                rx_bytes: 0,
                tx_bytes: 0,
            },
            diskio: DiskIo {
                read_bytes: 0,
                write_bytes: 0,
            },
            disk_space: DiskSpace {
                scanned_at: 0,
                roots: vec!["/".into()],
                top_dirs: vec![],
                top_files: vec![],
            },
            processes: Processes {
                count: 0,
                top_cpu: vec![],
                top_mem: vec![],
            },
        }
    }

    /// Start a one-shot HTTP server. It reads the request, writes `response`, then
    /// half-closes the write side so the client sees a clean end-of-response.
    fn serve_once(response: &'static str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&buf[..n]);
                if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header_end = pos + 4;
                    let headers = String::from_utf8_lossy(&data[..header_end]).to_lowercase();
                    let content_length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse().ok())
                        })
                        .unwrap_or(0);
                    if data.len() >= header_end + content_length {
                        break;
                    }
                }
            }
            let request = String::from_utf8_lossy(&data).to_string();
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
            let _ = stream.shutdown(Shutdown::Write);
            let _ = stream.read(&mut buf);
            request
        });
        (format!("http://{addr}/ingest"), handle)
    }

    #[test]
    fn posts_json_with_bearer_token() {
        let (endpoint, handle) = serve_once("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");

        let result = push(&endpoint, "s3cret", &sample());
        let request = handle.join().unwrap();
        let lower = request.to_lowercase();

        assert!(result.is_ok(), "push failed: {result:?}");
        assert!(lower.contains("bearer s3cret"), "missing token: {request}");
        assert!(lower.contains("\"hostname\""), "missing body: {request}");
    }

    #[test]
    fn reports_unexpected_status() {
        let (endpoint, handle) =
            serve_once("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");

        let err = push(&endpoint, "bad", &sample()).unwrap_err();
        let _ = handle.join().unwrap();
        assert!(err.contains("401"), "unexpected error: {err}");
    }
}
