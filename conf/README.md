# Personal Activity Index - Reverse Proxy Configurations

This directory contains example reverse proxy configurations for deploying the Personal Activity Index HTTP server behind nginx or Caddy.

## Quick Start

### Option 1: nginx

#### macOS

1. Install nginx:

   ```sh
   brew install nginx
   ```

2. Copy the configuration:

   ```sh
   # For localhost testing
   cp nginx.conf /opt/homebrew/etc/nginx/servers/pai.conf

   # Or symlink to keep it in sync
   ln -s $(pwd)/nginx.conf /opt/homebrew/etc/nginx/servers/pai.conf
   ```

3. Start the pai server:

   ```sh
   pai serve -a 127.0.0.1:8080
   ```

4. Start nginx:

   ```sh
   brew services start nginx
   ```

5. Access at <http://localhost>

#### Linux

1. Install nginx:

   ```sh
   # Debian/Ubuntu
   sudo apt install nginx

   # RHEL/Fedora
   sudo dnf install nginx
   ```

2. Copy the configuration:

   ```sh
   sudo cp nginx.conf /etc/nginx/sites-available/pai
   sudo ln -s /etc/nginx/sites-available/pai /etc/nginx/sites-enabled/
   ```

3. Start the pai server:

   ```sh
   pai serve -a 127.0.0.1:8080
   ```

4. Test and reload nginx:

   ```sh
   sudo nginx -t
   sudo systemctl reload nginx
   ```

5. Access at <http://localhost>

### Option 2: Caddy

#### macOS

1. Install Caddy:

   ```sh
   brew install caddy
   ```

2. Copy the Caddyfile:

   ```sh
   cp Caddyfile /opt/homebrew/etc/Caddyfile
   ```

3. Start the pai server:

   ```sh
   pai serve -a 127.0.0.1:8080
   ```

4. Start Caddy:

   ```sh
   brew services start caddy
   ```

5. Access at <http://localhost>

#### Linux

1. Install Caddy:

   ```sh
   # See https://caddyserver.com/docs/install

   # Debian/Ubuntu
   sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https
   curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
   curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
   sudo apt update
   sudo apt install caddy
   ```

2. Copy the Caddyfile:

   ```sh
   sudo cp Caddyfile /etc/caddy/Caddyfile
   ```

3. Start the pai server:

   ```sh
   pai serve -a 127.0.0.1:8080
   ```

4. Reload Caddy:

   ```sh
   sudo systemctl reload caddy
   ```

5. Access at <http://localhost>

## Production Deployment with Custom Domain

### nginx with SSL

1. Edit `nginx.conf` and replace `localhost` with your domain (e.g., `pai.example.com`)

2. Obtain SSL certificates using certbot:

   ```sh
   # macOS
   brew install certbot

   # Linux
   sudo apt install certbot python3-certbot-nginx  # Debian/Ubuntu
   sudo dnf install certbot python3-certbot-nginx  # RHEL/Fedora
   ```

3. Get certificates:

   ```sh
   sudo certbot --nginx -d pai.example.com
   ```

4. Certbot will automatically update your nginx configuration with SSL settings

5. Set up auto-renewal:

   ```sh
   # Test renewal
   sudo certbot renew --dry-run

   # On Linux, certbot sets up a systemd timer automatically
   # On macOS, add to crontab:
   sudo crontab -e
   # Add: 0 0 * * * certbot renew --quiet
   ```

### Caddy with Custom Domain

1. Edit `Caddyfile` and uncomment the production section

2. Replace `pai.example.com` with your actual domain

3. Ensure DNS A/AAAA records point to your server

4. Reload Caddy:

   ```sh
   sudo systemctl reload caddy  # Linux
   brew services restart caddy  # macOS
   ```

Caddy automatically obtains and renews SSL certificates from Let's Encrypt - no additional configuration needed!

## Running pai as a System Service

### macOS (launchd)

Create `/Library/LaunchDaemons/com.pai.server.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.pai.server</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/pai</string>
        <string>serve</string>
        <string>-a</string>
        <string>127.0.0.1:8080</string>
        <string>-d</string>
        <string>/var/lib/pai/pai.db</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/var/log/pai/stdout.log</string>
    <key>StandardErrorPath</key>
    <string>/var/log/pai/stderr.log</string>
    <key>WorkingDirectory</key>
    <string>/var/lib/pai</string>
</dict>
</plist>
```

Load the service:

```sh
sudo launchctl load /Library/LaunchDaemons/com.pai.server.plist
```

### Linux (systemd)

Create `/etc/systemd/system/pai.service`:

```ini
[Unit]
Description=Personal Activity Index
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/pai serve -a 127.0.0.1:8080 -d /var/lib/pai/pai.db
Restart=on-failure
RestartSec=5
User=pai
Group=pai
WorkingDirectory=/var/lib/pai

[Install]
WantedBy=multi-user.target
```

Create the pai user and directories:

```sh
sudo useradd -r -s /bin/false pai
sudo mkdir -p /var/lib/pai
sudo chown pai:pai /var/lib/pai
```

Enable and start the service:

```sh
sudo systemctl daemon-reload
sudo systemctl enable pai
sudo systemctl start pai
```

Check status:

```sh
sudo systemctl status pai
```

View logs:

```sh
sudo journalctl -u pai -f
```

## Testing

Verify the proxy is working:

```sh
# Health check
curl http://localhost/status

# API endpoint
curl http://localhost/api/feed?limit=5

# Specific item
curl http://localhost/api/item/some-item-id
```

## Additional Resources

- [nginx documentation](https://nginx.org/en/docs/)
- [Caddy documentation](https://caddyserver.com/docs/)
- [Let's Encrypt](https://letsencrypt.org/)
- [Personal Activity Index main documentation](../README.md)
- [Deployment guide](../DEPLOYMENT.md)
