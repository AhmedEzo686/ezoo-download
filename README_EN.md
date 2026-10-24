<div align="center">

**[ 🇸🇦 العربية ](README.md) • [ 🇬🇧 English ](README_EN.md)**

# Ezoo Download

A fast, native Windows download manager written from scratch in **Rust**.

Developed by **Ahmed Ezzat**

[![Windows](https://img.shields.io/badge/Windows-10%20%2C%2011-0078D6?style=flat-square&logo=windows11&logoColor=white)](https://github.com/AhmedEzo686/ezoo-download)
[![Rust](https://img.shields.io/badge/Made%20with-Rust-B7410E?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square)](LICENSE)
[![UI Languages](https://img.shields.io/badge/UI-8%20Languages-success?style=flat-square)](#-internationalization)

</div>

---

## 📌 Overview

**Ezoo Download** is a modern, high-performance desktop download manager for Windows, engineered in **Rust** with a responsive **Slint** user interface. It accelerates downloads by dividing files across multiple concurrent connections and provides robust pause-and-resume capabilities that survive network dropouts and application restarts.

The application features full multilingual support across **eight languages**: Arabic, English, French, Spanish, German, Portuguese, Turkish, and Russian.

> [!IMPORTANT]
> Users are responsible for complying with copyright laws and the terms of service of any third-party websites or services accessed through this software.

---

## ✨ Features

| Feature | Description |
|---|---|
| 🔀 **Multi-Connection Engine** | Splits file downloads across up to 32 parallel connections for maximum bandwidth utilization |
| ⏯️ **Pause & Resume** | Pause and resume downloads at any time, even after restarting the computer or reopening the app |
| 🌐 **Browser Integration** | Seamless companion browser extension with automatic link intercept and interactive video detection |
| 🎬 **Video & Audio Downloads** | High-quality media extraction from pages and playlists powered by yt-dlp & FFmpeg with resolution choice |
| 🗂️ **Smart Categorization** | Automatically organizes incoming downloads into dedicated folders (Video, Music, Documents, etc.) |
| ⏰ **Queues & Scheduling** | Queue prioritization, scheduled start/stop times, and post-download actions (shutdown, sleep, exit) |
| 🐢 **Speed Limiter** | Global and per-task bandwidth caps to prevent connection saturation |
| 🔎 **Site Grabber** | Crawls entire web pages to batch-download files, images, documents, or media |
| 📋 **Batch Downloads** | Add multiple URLs at once, import/export download lists in multiple formats |
| 🖱️ **Multi-Selection** | Intuitive selection via Ctrl, Shift, Ctrl+A, and mouse box dragging |
| 🎨 **9 Themes & Transparency** | Elegant light, dark, and colorful themes with customizable window opacity |
| 🌍 **8 Languages** | Native UI in Arabic, English, French, Spanish, German, Portuguese, Turkish, and Russian |

---

## 📦 Installation

Download the latest installer (`EzooDownload-Setup.exe`) from the **[GitHub Releases](https://github.com/AhmedEzo686/ezoo-download/releases)** page.

The setup installs per-user and does not require administrative privileges.

### Building from Source

To compile and build the installer locally on Windows 10 or 11:

1. Clone the repository:
   ```bash
   git clone https://github.com/AhmedEzo686/ezoo-download.git
   cd ezoo-download
   ```
2. Run the build script:
   ```cmd
   BUILD.bat
   ```
   Or invoke the PowerShell script directly:
   ```powershell
   .\build-windows.ps1
   ```
3. The generated installer will be located in the **`dist`** directory (`dist\EzooDownload-Setup.exe`).

---

## ⚙️ System Requirements

- **Windows 10 or Windows 11** (64-bit)
- Active internet connection

---

## 🔍 Architecture & Network Behavior

Transparency and user trust are core design principles of Ezoo Download:

| Component | Behavior |
|---|---|
| 🔄 **Auto-Update** | Checks `version.json` on GitHub Releases every 6 hours and at startup. Updates are verified via SHA-256 before installation. Update check can also be triggered manually via Help → Check for updates. |
| 🧰 **Helper Tools** | Leverages official standalone builds of `yt-dlp` and `FFmpeg` (placed in the `tools` folder), refreshed periodically from their GitHub releases. |
| 🔌 **Extension API** | Listens locally on `127.0.0.1` (default port 9614). Rejects external network traffic and requires custom authorization headers (`X-SwiftGet`). |
| 🍪 **Cookies & Auth** | Transmits site cookies and referrers locally between the browser extension and the engine strictly for the requested download to support authenticated sessions. Data never leaves your machine. |
| 📋 **Clipboard Monitor** | Inspects clipboard for downloadable file links and offers an add prompt. Can be toggled on/off in Settings. |
| 🚀 **Startup Integration** | Optional "Start with Windows" setting configured during installation or via Options. |
| 💾 **Data Storage** | Settings, history, and queues are stored locally in `%APPDATA%\Ezoo\EzooDownload\data`. |

> **Privacy Notice**: Ezoo Download does not collect, track, or transmit any telemetry, usage statistics, or user analytics.

---

## 🗂️ Project Structure

```
├─ crates/
│  ├─ engine/          # Download engine (segmentation, HLS, muxing, scheduler, queues)
│  └─ app/             # Desktop application (Slint UI, tray icon, local API, i18n)
│     ├─ ui/           # Slint interface markup and styles
│     └─ lang/         # UI localization files (.po and .json tables)
├─ extension/          # Manifest V3 browser extension (Chrome, Edge, Brave, Opera, Firefox)
├─ installer/          # Inno Setup installer scripts and configuration
├─ BUILD.bat           # Quick build launcher
└─ build-windows.ps1   # PowerShell automated build and packaging pipeline
```

---

## 🛡️ Antivirus & SmartScreen Notice

Because Ezoo Download binaries are not yet digitally signed with an expensive commercial code-signing certificate, Windows SmartScreen or certain antivirus utilities may show a standard reputation warning for newly released versions. The complete source code is open and available in this repository for full auditability.

---

## 🔐 Reporting Security Issues

If you discover a security vulnerability, please report it privately through the **Issues** tab on GitHub.

---

## 👨‍💻 Developer & Credits

**Ahmed Ezzat** — [Ezoo Tech](https://github.com/AhmedEzo686)

---

## © License

© 2026 **Ahmed Ezzat** — All rights reserved.

Licensed under the **MIT License**. Open-source third-party libraries and licenses are credited in [`THIRD-PARTY-NOTICES.txt`](THIRD-PARTY-NOTICES.txt).
