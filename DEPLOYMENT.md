# Personal Activity Index – Deployment Guide

This guide walks through two common reverse proxy setups for `pai serve`: **nginx** and **Caddy**. Both sections include native (host binary) instructions and optional Docker paths if you prefer containerized deployments.

## Prerequisites

1. Build the CLI binary:

   ```sh
   cargo build --release -p pai
   ```

   The binary will live at `target/release/pai`.

2. Prepare a configuration + database location. The default locations follow the XDG spec, but you can override them with `-C` (config dir) and `-d` (database path).

3. Run a sync at least once so the database has data:

   ```sh
   ./target/release/pai sync -C /etc/pai -d /var/lib/pai/pai.db -a
   ```

4. Start the server (example binds to localhost so the proxy terminates TLS):

   ```sh
   ./target/release/pai serve -d /var/lib/pai/pai.db -a 127.0.0.1:8080
   ```

## nginx Deployment

### Host Setup

1. Install nginx via your package manager (`apt`, `dnf`, `brew`, etc.).
2. Create a systemd service for `pai` (optional but recommended):

   ```ini
   [Unit]
   Description=Personal Activity Index
   After=network.target

   [Service]
   ExecStart=/usr/local/bin/pai serve -d /var/lib/pai/pai.db -a 127.0.0.1:8080
   Restart=on-failure
   User=pai
   Group=pai
   WorkingDirectory=/var/lib/pai

   [Install]
   WantedBy=multi-user.target
   ```

3. Enable and start it:

   ```sh
   sudo systemctl daemon-reload
   sudo systemctl enable --now pai.service
   ```

### nginx Config

Create `/etc/nginx/conf.d/pai.conf`:

```nginx
server {
    listen 80;
    server_name pai.example.com;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Reload nginx: `sudo nginx -s reload`.

### Optional: nginx via Docker

Use an `nginx` image + bind-mount config:

```yaml
services:
  pai:
    image: ghcr.io/your-namespace/pai:latest
    command: ["serve", "-d", "/data/pai.db", "-a", "0.0.0.0:8080"]
    volumes:
      - ./data:/data
    expose:
      - "8080"

  nginx:
    image: nginx:1.27
    volumes:
      - ./nginx.conf:/etc/nginx/conf.d/default.conf:ro
    ports:
      - "80:80"
    depends_on:
      - pai
```

`nginx.conf` should proxy to `http://pai:8080` instead of localhost.

## Caddy Deployment

### Host Setup

1. Install Caddy (<https://caddyserver.com/docs/install>).
2. Keep the same `pai` systemd service from above (or run manually).

### Caddyfile Example

Create `/etc/caddy/Caddyfile`:

```caddyfile
pai.example.com {
    reverse_proxy 127.0.0.1:8080
    encode gzip zstd
    header {
        Referrer-Policy "no-referrer-when-downgrade"
        X-Content-Type-Options "nosniff"
    }
}
```

Caddy automatically provisions TLS certificates with Let’s Encrypt. Reload with `sudo systemctl reload caddy`.

### Optional: Caddy + Docker Compose

```yaml
services:
  pai:
    image: ghcr.io/your-namespace/pai:latest
    command: ["serve", "-d", "/data/pai.db", "-a", "0.0.0.0:8080"]
    volumes:
      - ./data:/data
    expose:
      - "8080"

  caddy:
    image: caddy:2
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy_data:/data
      - caddy_config:/config
    ports:
      - "80:80"
      - "443:443"
    depends_on:
      - pai

volumes:
  caddy_data:
  caddy_config:
```

Use the same `Caddyfile` contents as above, but point `reverse_proxy` to `pai:8080`.

## Health Checks & Monitoring

- `GET /status` – lightweight JSON (`status`, total items, counts per `source_kind`). Ideal for load balancer health probes.
- `GET /api/feed?limit=1` ensures the server can read from SQLite and return real data.
- `GET /api/item/{id}` is handy for debugging a specific record.
- Consider wiring `/status` into nginx/Caddy health checks (`/healthz`) or your platform’s monitoring agents.

## Security Tips

- Bind the `pai serve` process to `127.0.0.1` and let the proxy handle TLS.
- Run the binary as an unprivileged user with read/write access only to the DB path.
- Regularly rotate TLS certificates (Caddy does this automatically; for nginx use certbot or similar).
