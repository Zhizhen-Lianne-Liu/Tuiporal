# Tuiporal

A Terminal User Interface (TUI) for [Temporal](https://temporal.io) workflow orchestration, written in Rust.

## Features

- **Workflow Management**: Parent-only list by default, with search, filters, pagination, and live updates
- **Execution Tree**: Folder-style child-workflow branches and activities, with live statuses, running indicators, and elapsed runtimes
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

### Open a workflow directly (for agents and testing)

Build once in the cloned repository:

```bash
cargo build
```

If you know a workflow ID, open its execution without searching the list:

```bash
./target/debug/tuiporal show --workflow-id my-workflow-id
```

In **iTerm2 on macOS**, add `--split` to open it beside the terminal session
that ran the command. Your agent can run the same command using its shell tool:

```bash
./target/debug/tuiporal show --workflow-id my-workflow-id --split
```

If you also know the run ID (a workflow ID can have multiple runs), pass it with
`--run-id my-run-id`. Without it, Tuiporal opens the latest run. The CLI and
Tuiporal must connect to the **same Temporal server and namespace**; they read
`~/.tuiporal/config.yaml` as described below. `--split` needs an iTerm2 session
and creates a new pane each time. Close a workflow with `Esc` (back to list),
then `q` (exit). Outside iTerm2, omit `--split` and run the command in the
terminal where you want the UI.

**Agent instruction you can paste:** “After starting a Temporal workflow, use
its returned workflow ID and run ID to run `tuiporal show --workflow-id ID
--run-id RUN_ID --split`, so I can watch it in iTerm2.” If you have not
installed Tuiporal into your PATH, use `./target/debug/tuiporal` instead of
`tuiporal` and run the command from this repository.

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
- The state column shows how long each workflow/activity ran; running timers keep ticking. Queued work has no runtime yet.
- `↑/↓` or `j/k` - Navigate the tree; `Enter` - Open selected child workflow; `ESC` - Return to parent/list
- `/` - Search names/IDs in the tree, `f` - Cycle All/Active/Failed/Done, `c` - Clear tree search/filter
- `Tab` - Switch between tree and event history; `Enter` in history - View structured JSON event attributes (including JSON inputs/results)
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
