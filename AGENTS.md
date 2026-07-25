# Panda Server Deployment

## Build Linux release in WSL

Do **not** compile on `lighthouse`; its Rust toolchain is intentionally removed.

Build in WSL Debian with Rust 1.95.0. Current rustc releases can ICE while
emitting the `dead_code` lint for this crate, so disable only that lint during
the build:

```bash
cd /mnt/z/source/lab/everbook/panda-server
env CARGO_TARGET_DIR=/home/yuhangch/panda-server-target-195 \
  RUSTFLAGS='-A dead_code' \
  cargo +1.95.0 build --release -p server
```

The Linux binary is:

```text
/home/yuhangch/panda-server-target-195/release/panda
```

## Publish to lighthouse

The remote service is `panda.service`; it runs `/root/panda/panda` with
working directory `/root/panda`. Use Windows-host SSH credentials (WSL does
not have the lighthouse key) to stream the WSL binary, back up the previous
binary, restart, and verify health:

```powershell
wsl.exe -d debian --exec /bin/bash -lc "cat /home/yuhangch/panda-server-target-195/release/panda" |
  ssh lighthouse "cat > /root/panda/panda.new"
Get-Content .\openapi.yaml -Raw | ssh lighthouse "cat > /root/panda/openapi.yaml.new"
ssh lighthouse "chmod 0755 /root/panda/panda.new; mv /root/panda/panda /root/panda/panda.previous; mv /root/panda/panda.new /root/panda/panda; mv /root/panda/openapi.yaml.new /root/panda/openapi.yaml; systemctl restart panda.service; systemctl is-active panda.service"
ssh lighthouse "curl -fsS --max-time 10 http://127.0.0.1:8787/api/v1/health"
```

Keep `/root/panda/panda.previous` for rollback. The remote login shell may
warn about a missing `/root/.cargo/env.fish`; this is expected after removing
the server Rust toolchain and does not affect `panda.service`.
