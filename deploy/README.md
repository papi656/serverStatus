# Deploying the server-status agent

## 1. Build a static binary (on your workstation, not the VPS)

A release Rust link can peak at 1-2 GB RAM, so avoid building on a small VPS:

```bash
rustup target add x86_64-unknown-linux-musl

# On macOS, point Cargo at rust-lld (Apple's clang cannot link ELF).
# On Linux with musl-tools installed, you can omit this line.
RUST_LLD="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/rust-lld"

CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="$RUST_LLD" \
  RUSTFLAGS="-C strip=symbols" \
  cargo build --release --target x86_64-unknown-linux-musl -p server-status-agent
```

## 2. Create the service user and install the binary

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin serverstatus
sudo install -m 0755 \
  target/x86_64-unknown-linux-musl/release/server-status-agent \
  /usr/local/bin/server-status-agent
```

## 3. Configure the environment

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

### Changing the endpoint later (no rebuild)

`ENDPOINT` (and every other setting) is read from the environment at startup — it is **not**
baked into the binary. To point the agent somewhere else, edit the file and restart:

```bash
sudo nano /etc/server-status-agent.env     # change the ENDPOINT= line
sudo systemctl restart server-status-agent
```

> The build is **HTTP only** (TLS is disabled), so `ENDPOINT` must be a plain `http://` URL.

## 4. Install and start the unit

```bash
sudo install -m 0644 deploy/server-status-agent.service /etc/systemd/system/
sudo install -m 0644 deploy/logrotate-server-status-agent /etc/logrotate.d/server-status-agent
sudo systemctl daemon-reload
sudo systemctl enable --now server-status-agent
```

## 5. Verify the capability took effect

```bash
systemctl show server-status-agent -p AmbientCapabilities
getpcaps "$(pgrep -f server-status-agent)"
```

Both should list `CAP_DAC_READ_SEARCH`. If they do not, ensure the unit does **not**
set `NoNewPrivileges=yes` (which can strip ambient capabilities), then
`sudo systemctl restart server-status-agent`.

## 6. Check the logs

The agent logs only startup and push failures, appended to
`/var/log/server-status-agent.log`:

```bash
sudo tail -f /var/log/server-status-agent.log
```

A healthy start logs one line:

```
[2026-10-12T13:42:01Z] INFO started host=vps-1 endpoint=http://127.0.0.1:8099/ingest interval=60s scan_interval=43200s
```

If the endpoint is unreachable you get one `ERROR push failed: …` line when it starts
failing, and one `INFO push recovered …` line when it succeeds again. Rotation is handled
by `/etc/logrotate.d/server-status-agent`.
