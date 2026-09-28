# Tuiporal

A Terminal User Interface (TUI) for [Temporal](https://temporal.io) workflow orchestration, written in Rust.

## Features

- **Workflow Management**: Parent-only list by default, with search, filters, pagination, and live updates
- **Execution Tree**: Folder-style child-workflow branches and activities, with animated running indicators and live running/queued/completed states
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
git clone --recurse-submodules https://github.com/Zhizhen-Lianne-Liu/Tuiporal.git
cd tuiporal
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
- `1` - Workflows, `2` - Namespaces, `?` - Help, `q` - Quit/back

### Workflows Screen
- `↑/↓` or `j/k` - Navigate, `Enter` - View details
- `/` - Search, `f` - Filter by status, `c` - Clear search/status filter
- `v` - Toggle parent-only (default) / all workflows, including children
- `r` - Refresh, `a` - Toggle auto-refresh
- `n/p` - Next/Previous page

### Workflow Detail
- A compact execution header and folder-style tree open by default; statuses refresh every 5 seconds. Running and queued work is highlighted.
- `↑/↓` or `j/k` - Navigate the tree; `Enter` - Open selected child workflow; `ESC` - Return to parent/list
- `Tab` - Switch between tree and raw event history; `Enter` in history - View event details
- `r` - Refresh now, `a` - Toggle auto-refresh
- `t` - Terminate, `x` - Cancel, `s` - Signal the selected workflow (confirmation shows its ID)

Child relationships use Temporal execution history and `RootWorkflowId` visibility; live activity states come from `DescribeWorkflowExecution`. Child histories outside the current namespace may appear as placeholders.

### Namespaces
- `↑/↓` or `j/k` - Navigate, `Enter` - Switch namespace
- `r` - Refresh, `ESC` - Back

## Prerequisites

- A recent stable Rust toolchain (tested with 1.97)
- Protocol Buffers compiler (`protoc`)
  - macOS: `brew install protobuf`
  - Linux: `sudo apt-get install protobuf-compiler`

## Building

```bash
git clone --recurse-submodules https://github.com/Zhizhen-Lianne-Liu/Tuiporal.git
cd tuiporal
cargo build --release
```

The Temporal API is pinned as a Git submodule. For an existing clone, run `git submodule update --init`. The build script generates Rust bindings into Cargo’s build directory; no generated source files need to be committed.

## Development

```bash
# Run with logging
TUIPORAL_LOG=/tmp/tuiporal.log RUST_LOG=debug cargo run

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
