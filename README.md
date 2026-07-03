# KeyPulse

KeyPulse is a privacy-first keyboard activity dashboard for macOS and Windows.

It helps you understand typing rhythm, shortcut usage, and daily keyboard activity without storing typed text, raw key sequences, passwords, or ordinary letter/number values.

[中文说明](./README.zh-CN.md)

## Features

- Real-time keystrokes per minute
- Daily total key count
- Peak keys per minute
- Hourly activity chart
- Key category statistics for Enter, Backspace, Tab, Esc, arrows, function keys, modifiers, and more
- Shortcut statistics for combinations such as Cmd+C or Ctrl+V
- macOS menu bar resident mode after closing the main window
- Local-only storage for aggregate statistics

## Privacy

KeyPulse is designed to avoid collecting sensitive text.

- It does not store typed text.
- It does not store raw key sequences.
- It does not store passwords.
- Ordinary letters and numbers are counted only as categories.
- Statistics are stored locally on your device.

## Platform Notes

### macOS

macOS requires Input Monitoring permission before KeyPulse can receive global keyboard events.

Open:

```text
System Settings > Privacy & Security > Input Monitoring
```

Enable KeyPulse, then restart the app if the permission state does not update immediately.

### Windows

The MVP is designed to support Windows builds as well. Some security software may require allowing KeyPulse to listen to global keyboard events.

## Development

Requirements:

- Node.js
- Rust
- Tauri prerequisites for your platform

Install dependencies:

```bash
npm install
```

Run in development mode:

```bash
npm run tauri:dev
```

Build frontend assets:

```bash
npm run build
```

Build the desktop app:

```bash
npm run tauri -- build
```

## Project Structure

```text
keypulse-mac/
├── src/                 # React renderer
├── src-tauri/           # Tauri and Rust native layer
├── scripts/             # Utility scripts
├── package.json
└── README.md
```

## License

MIT
