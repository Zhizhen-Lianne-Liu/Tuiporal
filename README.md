# Tuiporal

A Terminal User Interface (TUI) for [Temporal](https://temporal.io) workflow orchestration, written in Rust.

## Features

- **Workflow Management**: List, search, filter, and view workflows with real-time updates
- **Workflow Operations**: Terminate, cancel, and signal workflows
- **Namespace Management**: Browse and switch between namespaces
- **Authentication**: Temporal Cloud (API key + TLS) and mTLS support
- **Modern UI**: Vim-style navigation, animated indicators, color-coded status

## Quick Start

### Local Development

```bash
# Start Temporal server
docker run -d -p 7233:7233 temporalio/auto-setup:latest

# Clone and build
git clone https://github.com/yourusername/tuiporal.git
cd tuiporal
git clone --depth 1 https://github.com/temporalio/api.git proto/temporal-api
cargo run
```

### Temporal Cloud

Create `~/.tuiporal/config.yaml`:

```yaml
active_profile: cloud

profiles:
  - name: cloud
    address: yournamespace.a2dd6.tmprl.cloud:7233
    namespace: yournamespace.a2dd6
    api_key: your-api-key-here
    tls:
      enabled: true
```

Get your API key from [Temporal Cloud Console](https://cloud.temporal.io) → Settings → API Keys.

## Configuration

Configuration file: `~/.tuiporal/config.yaml`

**Local Server (no auth)**:
```yaml
profiles:
  - name: local
    address: localhost:7233
    namespace: default
```

**mTLS (client certificates)**:
```yaml
profiles:
  - name: production
    address: temporal.example.com:7233
    namespace: production
    tls:
      enabled: true
      cert_path: /path/to/client-cert.pem
      key_path: /path/to/client-key.pem
      ca_path: /path/to/ca-cert.pem
```

**Multiple profiles**:
```yaml
active_profile: local

profiles:
  - name: local
    address: localhost:7233
    namespace: default

  - name: cloud
    address: dev.a2dd6.tmprl.cloud:7233
    namespace: dev.a2dd6
    api_key: your-key
    tls:
      enabled: true
```

## Keybindings

### Global
- `1` - Workflows, `2` - Namespaces, `h/?` - Help, `q` - Quit

### Workflows Screen
- `↑/↓` or `j/k` - Navigate, `Enter` - View details
- `/` - Search, `f` - Filter by status, `c` - Clear filters
- `r` - Refresh, `a` - Toggle auto-refresh
- `n/p` - Next/Previous page

### Workflow Detail
- `Tab` - Switch tabs, `↑/↓` or `j/k` - Scroll
- `t` - Terminate, `x` - Cancel, `s` - Signal
- `ESC` - Back

### Namespaces
- `↑/↓` or `j/k` - Navigate, `Enter` - Switch namespace
- `r` - Refresh, `ESC` - Back

## Prerequisites

- Rust 1.70+
- Protocol Buffers compiler (`protoc`)
  - macOS: `brew install protobuf`
  - Linux: `sudo apt-get install protobuf-compiler`

## Building

```bash
git clone https://github.com/yourusername/tuiporal.git
cd tuiporal
git clone --depth 1 https://github.com/temporalio/api.git proto/temporal-api
cargo build --release
```

## Development

```bash
# Run with logging
RUST_LOG=debug cargo run

# Format and lint
cargo fmt
cargo clippy
```

## License

Apache License 2.0

## Acknowledgments

- [Temporal](https://temporal.io) - Workflow orchestration platform
- [Tempo](https://github.com/galaxy-io/tempo) - Go-based TUI inspiration
- [Ratatui](https://ratatui.rs) - Rust TUI library
