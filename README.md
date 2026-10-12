# serverStatus — VPS status agent

A small, native **Rust** agent that runs on a Linux VPS, collects host metrics, and pushes
them as a JSON snapshot to an HTTP endpoint (a status dashboard). It is designed to be
lightweight: a single static binary, no runtime, no metrics library, no async runtime.

It reads the host directly from `/proc` and `statvfs`, so it must be run **natively on the
machine** (not inside a container) to see the real CPU, memory, disk, and process data.

> The dashboard (receiver + web UI) lives in a separate repository. This repo is only the
> agent that produces and sends the data.

## What it collects

- **CPU** — total and per-core utilisation
- **Memory** — total, used, available, swap
- **Load average** — 1 / 5 / 15 minute
- **Uptime**
- **Disks** — per-mount total/used for real block devices
- **Network** — cumulative RX/TX byte counters
- **Disk I/O** — cumulative read/write byte counters
- **Processes** — count, plus top 10 by CPU and top 10 by memory (name only, never the
  command line)
- **Disk space** — top 10 directories and top 10 files by size (scanned every 12 hours)

Sample rate defaults to 60 s; the disk-space scan runs every 12 h.

## Requirements

- **Build machine:** Rust (stable) and Cargo. [rustup.rs](https://rustup.rs) is the easiest
  way to install both.
- **Target machine (VPS):** Linux, `x86_64` (or adapt the target triple), with `systemd`.

## 1. Build the binary

The recommended binary is a **statically linked musl** build — no shared-library
dependencies, so it runs on any glibc/musl Linux.

### Install the Rust toolchain (if you don't have it)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
```

### Build a static Linux binary

> **Building from a Mac?** A plain `cargo build` fails: the musl target needs an ELF
> linker, and Apple's `clang` can't link ELF. The command below points Cargo at
> `rust-lld`, which ships with Rust — no extra toolchain to install.

```bash
# Add the static Linux target once (x86_64 = most VPS; use
# aarch64-unknown-linux-musl for ARM64 hosts)
rustup target add x86_64-unknown-linux-musl

# Locate the rust-lld linker shipped with the Rust toolchain
RUST_LLD="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/rust-lld"

# Build (statically linked, stripped)
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="$RUST_LLD" \
  RUSTFLAGS="-C strip=symbols" \
  cargo build --release --target x86_64-unknown-linux-musl -p server-status-agent
```

The output is a ~1 MB statically linked binary:

```
target/x86_64-unknown-linux-musl/release/server-status-agent
```

> **Building on Linux instead?** With `musl-tools` installed (giving you
> `x86_64-linux-musl-gcc`) you can drop the linker line. Building for the same machine
> (glibc) is just `cargo build --release -p server-status-agent`.

> **Do not build on a small VPS.** A release Rust link can peak at 1-2 GB of RAM. Build on
> your workstation (or CI) and copy the finished binary over.

## 2. Send the binary to the server

```bash
scp target/x86_64-unknown-linux-musl/release/server-status-agent \
    you@your-server:/tmp/server-status-agent
```

## 3. Run it on the server

SSH in and set it up as a systemd service.

### 3a. Create a service user and install the binary

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin serverstatus
sudo install -m 0755 /tmp/server-status-agent /usr/local/bin/server-status-agent
```

### 3b. Write the configuration

```bash
sudo tee /etc/server-status-agent.env >/dev/null <<'EOF'
ENDPOINT=http://127.0.0.1:8099/ingest
TOKEN=replace-with-the-shared-secret
INTERVAL_SECS=60
TOP_N=10
DISK_SCAN_INTERVAL_SECS=43200
DISK_SCAN_DEPTH=2
DISK_SCAN_ROOTS=/
EOF
sudo chmod 0600 /etc/server-status-agent.env
```

### 3c. Install the systemd unit

Copy [`deploy/server-status-agent.service`](deploy/server-status-agent.service) to the
server and install it:

```bash
sudo install -m 0644 deploy/server-status-agent.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now server-status-agent
```

### 3d. Verify it is running

```bash
systemctl status server-status-agent
journalctl -u server-status-agent -f
```

You should see a `pushed ts=<unix seconds>` line every interval. A `push failed …` line
means the endpoint or token is wrong.

### 3e. Verify the disk-scan capability

The 12-hour disk scan needs to read into root-only directories. The unit grants exactly one
capability, `CAP_DAC_READ_SEARCH`, to the otherwise-unprivileged service:

```bash
systemctl show server-status-agent -p AmbientCapabilities
getpcaps "$(pgrep -f server-status-agent)"
```

Both should list `CAP_DAC_READ_SEARCH`. If they don't, make sure the unit does **not** set
`NoNewPrivileges=yes` (it can strip ambient capabilities), then restart the service.

## 4. Test it

The binary has two test flags:

| Flag | What it does |
|---|---|
| `--once` | Collect **one** snapshot and send it to `ENDPOINT`, then exit |
| `--dry-run` | Print the snapshot JSON to stdout and exit **without** sending anything |

### 4a. Print a snapshot without sending it

```bash
TOKEN=dummy ./server-status-agent --once --dry-run
```

This prints the full JSON payload so you can inspect exactly what would be sent. (On
macOS there is no `/proc`, so the host metrics will be zero — run this on the VPS for real
values.)

### 4b. Send one snapshot to an endpoint

This is the end-to-end test: it actually POSTs the snapshot to `ENDPOINT`.

```bash
TOKEN=your-shared-secret \
ENDPOINT=http://127.0.0.1:8099/ingest \
./server-status-agent --once
```

A successful push prints `pushed ts=…` and exits. This is the same request the agent makes
on every normal interval, so if this works, the agent is wired up correctly.

### 4c. Test against a throwaway local endpoint

If you don't have the dashboard running yet, start a tiny listener that prints whatever it
receives, and point the agent at it.

**Terminal 1 — a one-shot receiver on port 8099:**

```bash
python3 - <<'PY'
from http.server import BaseHTTPRequestHandler, HTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length).decode()
        print("AUTH:", self.headers.get("Authorization"))
        print("BODY:", body)
        self.send_response(204)
        self.end_headers()

HTTPServer(("127.0.0.1", 8099), Handler).serve_forever()
PY
```

**Terminal 2 — send one snapshot to it:**

```bash
TOKEN=test-secret ENDPOINT=http://127.0.0.1:8099/ingest \
  ./server-status-agent --once
```

Terminal 1 should print `AUTH: Bearer test-secret` and the JSON snapshot.

You can also test against the real dashboard with `curl` to confirm the endpoint accepts
your payload:

```bash
curl -i -X POST http://127.0.0.1:8099/ingest \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"ts":0,"hostname":"test"}'
```

## 5. Configuration reference

All configuration is via environment variables (the systemd unit loads them from
`/etc/server-status-agent.env`).

| Variable | Meaning | Default |
|---|---|---|
| `ENDPOINT` | Ingest URL to POST snapshots to | `http://127.0.0.1:8099/ingest` |
| `TOKEN` | Bearer token sent as `Authorization: Bearer <TOKEN>` | **required** |
| `INTERVAL_SECS` | Seconds between snapshots | `60` |
| `TOP_N` | Processes per top list | `10` |
| `DISK_SCAN_INTERVAL_SECS` | Seconds between disk-space scans | `43200` (12 h) |
| `DISK_SCAN_DEPTH` | Directory depth for the top-dirs list | `2` |
| `DISK_SCAN_ROOTS` | Comma-separated roots to scan | `/` |
| `HOSTNAME` | Hostname reported in the payload | OS hostname |

> **HTTP only.** This build disables TLS (the agent talks to the dashboard over loopback),
> so `ENDPOINT` must be a plain `http://` URL. An `https://` endpoint will fail.

## 6. The ingest contract

The agent sends `POST <ENDPOINT>` with:

- Header `Authorization: Bearer <TOKEN>`
- Header `Content-Type: application/json`
- Body: one JSON snapshot

The receiver should reply `204 No Content` (or `200`) on success. Any other status, or a
connection error, is logged as `push failed` and the agent retries on the next interval.

A snapshot looks like:

```jsonc
{
  "ts": 1730000000,
  "hostname": "vps-1",
  "uptime_s": 812345,
  "load": [0.42, 0.55, 0.61],
  "cpu": { "total_pct": 12.3, "per_core_pct": [10.1, 15.0, 8.7] },
  "mem": { "total": 8589934592, "used": 3221225472, "available": 5368709120,
           "swap_total": 0, "swap_used": 0 },
  "disks": [ { "mount": "/", "total": 53687091200, "used": 21474836480 } ],
  "net":    { "rx_bytes": 123456789, "tx_bytes": 987654321 },
  "diskio": { "read_bytes": 55555555, "write_bytes": 44444444 },
  "disk_space": {
    "scanned_at": 1729996000,
    "roots": ["/"],
    "top_dirs":  [ { "path": "/var/lib", "bytes": 21474836480 } ],
    "top_files": [ { "path": "/var/log/syslog", "bytes": 5368709120 } ]
  },
  "processes": {
    "count": 132,
    "top_cpu": [ { "pid": 812, "name": "postgres", "cpu_pct": 4.1, "mem_pct": 22.4 } ],
    "top_mem": [ { "pid": 812, "name": "postgres", "cpu_pct": 4.1, "mem_pct": 22.4 } ]
  }
}
```

`net` and `diskio` are cumulative counters: the receiver derives rates between consecutive
samples.

## 7. Security notes

- The agent makes only **outbound** connections; it opens no listening port.
- Process command lines are **never** collected or sent — only process names.
- The service runs as an unprivileged user. Its only elevated privilege is the single
  `CAP_DAC_READ_SEARCH` capability, needed to size root-only directories. It cannot write,
  delete, `chown`, mount, or signal anything.
- The bearer token lives only in `/etc/server-status-agent.env` (mode `0600`); it is never
  committed to the repository.

## 8. Uninstall

```bash
sudo systemctl disable --now server-status-agent
sudo rm /etc/systemd/system/server-status-agent.service /etc/server-status-agent.env \
        /usr/local/bin/server-status-agent
sudo userdel serverstatus
sudo systemctl daemon-reload
```

## Repository layout

```
Cargo.toml                 workspace
shared/                    JSON wire-contract types
agent/                     the agent: /proc parsers, collector, disk scan, HTTP push
deploy/                    systemd unit + deploy notes
```
