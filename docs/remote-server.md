# Remote transcription server (`--serve`)

Handy can split its workload across two machines: a **server** with a capable GPU
loads the model and exposes it over HTTP, and a **client** captures audio and
sends it over the network for transcription. This is useful when the device you
dictate on (e.g. a laptop with a weak GPU) can't run a large model fast enough,
but another machine on your network (or over [Tailscale](https://tailscale.com))
can.

```
┌──────────────────────┐   HTTP (Tailscale/LAN)   ┌──────────────────────┐
│  Client (laptop)     │  ───────────────────────▶│  Server (GPU box)    │
│  capture + VAD       │   f32 16 kHz mono        │  handy --serve       │
│  paste result        │  ◀───────────────────────│  Whisper / Parakeet  │
└──────────────────────┘   transcribed text       └──────────────────────┘
```

Traffic is plain HTTP — rely on Tailscale's WireGuard tunnel (or another trusted
transport) for encryption. A shared Bearer token adds an application-layer guard.

## 1. Build on the server (GPU box, over SSH)

```bash
git clone https://github.com/<your-fork>/Handy && cd Handy
bun install
bun run tauri build            # produces src-tauri/target/release/handy
```

See [`BUILD.md`](../BUILD.md) for platform build prerequisites (Vulkan/OpenBLAS
on Linux).

## 2. Install + configure the service

```bash
sudo useradd -r -s /usr/sbin/nologin -d /var/lib/handy -m handy
sudo install -m 0755 src-tauri/target/release/handy /usr/local/bin/handy
sudo install -m 0644 packaging/handy-server.service /etc/systemd/system/
sudo systemctl daemon-reload
```

Configure via a drop-in (no edit to the unit file):

```bash
sudo systemctl edit handy-server
```

```ini
[Service]
Environment="HANDY_TOKEN=change-me-please"
Environment="HANDY_BIND=0.0.0.0:8080"
Environment="HANDY_MODEL=handy-computer/parakeet-unified-en-0.6b-gguf"
Environment="HANDY_LANGUAGE=fr"
Environment="RUST_LOG=info"
```

| Var              | Purpose                                                              |
| ---------------- | -------------------------------------------------------------------- |
| `HANDY_BIND`     | `host:port` to bind. `0.0.0.0:8080` exposes on all interfaces.       |
| `HANDY_TOKEN`    | Shared Bearer secret the client must send. Optional but recommended. |
| `HANDY_MODEL`    | Model id (see `handy --list-models`) to load.                        |
| `HANDY_LANGUAGE` | Language hint for transcription (e.g. `fr`, `en`, `auto`).           |

## 3. Provision the model (optional)

The server auto-downloads the configured model on first run, but you can fetch
it explicitly and watch progress in the journal:

```bash
sudo -u handy /usr/local/bin/handy --list-models
sudo -u handy /usr/local/bin/handy --download-model <model-id>
```

## 4. Start + verify

```bash
sudo systemctl enable --now handy-server
sudo systemctl status handy-server
journalctl -u handy-server -f

# Smoke-test the API directly on the server:
curl http://127.0.0.1:8080/health
```

## 5. Configure the client (your laptop)

In Handy → **Settings → Transcription Server**:

1. Set **Inference backend** to **Remote server**.
2. Set **Server URL** to the server's address — its Tailscale IP
   (`http://100.x.y.z:8080`) or MagicDNS name (`http://gpu-box:8080`).
3. Set **Auth token** to the `HANDY_TOKEN` value.
4. Click **Test connection** — you should see the server's loaded model.

Now dictation on the laptop is transcribed on the server and the text is pasted
locally.

## Troubleshooting

- **`Model is not loaded` / `loaded: false` in /health** — the server hasn't
  finished loading/downloading the model yet; check `journalctl -u handy-server`.
- **401 Unauthorized** — the client's token doesn't match `HANDY_TOKEN`.
- **Connection refused** — the server binds loopback by default; set
  `HANDY_BIND=0.0.0.0:8080` and confirm the port is reachable over Tailscale
  (`tailscale status`, and Tailscale ACLs allow the port).
- **Slow first request** — the model loads lazily; it's preloaded at startup but
  the very first cold load still takes time. Subsequent requests reuse it.

## Protocol

- `GET /health` → `{ status, model?, loaded, engine? }`
- `POST /transcribe` — body = raw little-endian f32 bytes (16 kHz mono), headers
  `X-Sample-Rate`, `X-Language`, `Authorization: Bearer <token>` →
  `{ text, language? }`
- `GET /status` — alias of `/health`.

The server transcribes with **its own** configured model and language (single
source of truth). The client sends `X-Language` for diagnostics; per-request
language override is not honoured in v1.
