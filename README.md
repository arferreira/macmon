# macmon

A blazing fast terminal-based performance monitor for macOS with intelligent cleanup capabilities.

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![macOS](https://img.shields.io/badge/mac%20os-000000?style=for-the-badge&logo=macos&logoColor=F0F0F0)

## Features

- **Real-time System Monitoring**
  - CPU usage
  - RAM usage
  - Disk usage
  - Swap usage
  - Live process monitoring

- **Intelligent Issue Detection**
  - Scans for large `node_modules` directories
  - Lists Docker images taking up space
  - Identifies resource-heavy processes

- **Interactive Cleanup**
  - Remove unused `node_modules` directories
  - Prune Docker images
  - Clean Homebrew cache
  - Kill memory-hungry processes to free RAM

- **Beautiful TUI**
  - Clean, intuitive interface built with [ratatui](https://github.com/ratatui-org/ratatui)
  - Real-time updates
  - Color-coded status indicators

## Installation

### Prerequisites

- macOS
- Rust toolchain (install from [rustup.rs](https://rustup.rs))

### Build from source

```bash
git clone https://github.com/arferreira/macmon.git
cd macmon
cargo build --release
```

The binary will be available at `target/release/macmon`

### Install globally

```bash
cargo install --path .
```

## Usage

Simply run:

```bash
macmon
```

### Controls

- `c` - Open cleanup menu
- `↑/↓` or `j/k` - Navigate menus
- `Enter` - Execute selected action
- `Esc` - Go back/cancel
- `q` - Quit

## Screenshots

```
┌─ Mac Health Monitor ─────────────────────────────┐
│ Disk: [████████░░] 80% (200GB/250GB) ⚠️          │
│ RAM:  [██████░░░░] 60% (9.6GB/16GB)  ✓          │
│ CPU:  [███░░░░░░░] 30% avg          ✓          │
│ Swap: [██░░░░░░░░] 2.1GB           ⚠️          │
├──────────────────────────────────────────────────┤
│ Top Issues:                                      │
│ • node_modules: 45GB in 23 projects             │
│ • Docker images: 18 found                       │
│ • Top processes by resource usage:              │
├──────────────────────────────────────────────────┤
│ [c] Clean  [q] Quit                             │
└──────────────────────────────────────────────────┘
```

## Safety

**⚠️ Warning:** The cleanup operations are destructive and permanent:

- Deleting `node_modules` cannot be undone (you can reinstall with `npm install`)
- Killing processes with `kill -9` forces termination without saving
- Always review what you're deleting before confirming

## Contributing

Contributions are welcome! Feel free to:

- Report bugs
- Suggest new features
- Submit pull requests

## License

MIT

## Author

Created with ☕ by [@arferreira](https://github.com/arferreira)
