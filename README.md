# LocalDrop

LocalDrop is a local cross-platform file transfer tool that runs a local HTTP server on a Windows PC and allows mobile devices to stream files to and from the PC over a local Wi-Fi network.

## Features

- **Embedded HTTP Server**: Runs an `axum` HTTP server on the Windows backend (Tauri).
- **QR Code Pairing**: Generates a QR code on the desktop app for mobile devices to scan.
- **Zero-Buffering Streaming**: Files (including multi-GB files) stream directly between the mobile device and the Windows disk to minimize memory usage.
- **Progress Tracking**: Real-time upload progress (speed and percentage) is shown on both the mobile web interface and the desktop app.
- **Multi-File Upload**: Select multiple files on your phone or PC for batch transfers.

## Architecture

- **Backend**: Rust using Tauri v2, `axum` for HTTP, and `tokio` for async streaming.
- **Desktop Frontend**: Vanilla HTML/JS/CSS.
- **Mobile Frontend**: A mobile-friendly web page served directly from the Rust backend.

## Installation

Download the latest executable here: [Local-drop.exe](https://github.com/Chauhan-saab2006/LocalDropt-APP/raw/main/Local-drop.exe)

## Getting Started


1. Navigate to `windows-app`:
   ```bash
   cd windows-app
   ```
2. Run the development server:
   ```bash
   npx tauri dev
   ```
3. To build a standalone executable or installer:
   ```bash
   npx tauri build
   ```

## Usage

1. Open the Windows application.
2. Ensure both the PC and phone are on the same Wi-Fi network.
3. Scan the QR code with your phone or navigate to the displayed URL.
4. Upload files from the phone to the PC or download shared files from the PC to the phone.



 ## Code Signing
   See [CODE_SIGNING.md](.signpath/CODE_SIGNING.md) for how LocalDrop signs its Windows releases.