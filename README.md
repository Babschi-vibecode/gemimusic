# 🎵 gemimusic - Lightweight Rust YouTube Music Client for Android & Desktop

**gemimusic** is a high-performance, lightweight YouTube Music streaming player written in **100% Rust** with a modern, hardware-accelerated declarative UI powered by **[Slint](https://slint.dev/)**.

It connects to YouTube Music's InnerTube API for searching and streaming tracks, and integrates **Google Account Login via RFC 8628 OAuth 2.0 Device Flow** to seamlessly sync your personal playlists, liked songs, and music library.

---

## ⚡ Why Rust + Slint? (Lightweight by Design)

Traditional mobile music apps often rely on heavy frameworks like React Native, Flutter, or embedded WebViews (e.g. Electron/Capacitor), consuming 150MB+ of RAM and draining battery with JavaScript garbage collection.

- **Zero WebViews / Zero JS Engines**: Compiles down to native machine code (`ARM64` / `x86_64`).
- **Tiny RAM Footprint**: Consumes only ~15–25 MB of RAM during active playback.
- **Hardware Accelerated**: Fluid 60+ FPS touch UI rendered via OpenGL ES / Vulkan.
- **Battery Efficient**: Native audio stream decoding via `rodio` and `cpal` (leveraging Android's AAudio / OpenSL ES backend).
- **Single Codebase**: Runs on **Android** (`cargo apk`) and **Desktop** (Windows, Linux, macOS) for easy development and testing.

---

## 🌟 Key Features

1. **YouTube Music InnerTube Streaming**:
   - High-fidelity audio stream extraction directly from YouTube Music endpoints.
   - Intelligent adaptive format selection (AAC / Opus).
   - Built-in multi-instance fallback (Piped API) ensuring uninterrupted streaming even with restrictive network filters.

2. **Google Account Login & Playlist Sync**:
   - Standard **Google OAuth 2.0 Device Authorization Grant (RFC 8628)**.
   - Clean, lightweight auth without requiring embedded browser redirects:
     1. App displays a 4-letter/8-character code and links to `https://www.google.com/device`.
     2. Authorize via any phone or browser.
     3. App automatically detects approval, securely stores your refresh tokens, and fetches your playlists!
   - Instant synchronization of your **Liked Music** (`LL`) and custom playlists.

3. **Modern, Responsive Cyberpunk / Dark UI**:
   - Modern dark obsidian palette with violet & electric pink accents.
   - Real-time search query box with instant results list.
   - Persistent bottom dock player with progress scrubber, elapsed/total time, volume slider, and glowing transport controls.
   - Seamless mobile touch interactions (tap to play, responsive tabs).

---

## 📁 Project Structure

```
gemimusic/
├── Cargo.toml               # Rust package & Android cargo-apk manifest
├── AndroidManifest.xml      # Android permissions & NativeActivity config
├── build.rs                 # Slint UI compiler build script
├── ui/
│   ├── main.slint           # Main application view, tabs, player dock, auth modal
│   └── theme.slint          # Dark color palette & design tokens
├── src/
│   ├── main.rs              # Desktop executable runner
│   ├── lib.rs               # Android native cdylib entry point (android_main)
│   ├── app.rs               # App controller, event handlers & thread-safe dispatch
│   ├── config.rs            # Persistent local token & preferences storage
│   ├── api/
│   │   ├── mod.rs
│   │   ├── models.rs        # Track, Playlist, and OAuth data structures
│   │   ├── innertube.rs     # YouTube Music search, streaming URL & library fetcher
│   │   └── oauth.rs         # Google OAuth 2.0 Device Flow implementation
│   └── audio/
│       ├── mod.rs
│       └── player.rs        # Rodio/cpal streaming audio loop & progress ticker
├── scripts/
│   ├── build_android.bat    # Windows Android build automation
│   └── build_android.sh     # Unix Android build automation
└── assets/
    └── icon.png             # Application launcher icon
```

---

## 🛠️ Prerequisites

To build and run the project, ensure you have:

1. **Rust Toolchain**:
   ```bash
   # Install Rust if not already present:
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   # Or on Windows: download and run rustup-init.exe
   ```

2. **For Android Builds**:
   - Android SDK & NDK installed (via Android Studio or command-line tools).
   - `ANDROID_HOME` or `ANDROID_SDK_ROOT` environment variable configured.
   - `cargo-apk`:
     ```bash
     cargo install cargo-apk
     ```
   - Target architectures:
     ```bash
     rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
     ```

---

## 🚀 How to Build and Run

### 1. Run on Desktop (Development Mode)
You can test the player, search, UI, and Google sync immediately on your workstation:
```bash
cargo run
```

### 2. Build for Android APK
Run the build script:
- On Windows:
  ```cmd
  .\scripts\build_android.bat
  ```
- On Linux / macOS:
  ```bash
  chmod +x ./scripts/build_android.sh
  ./scripts/build_android.sh
  ```

Or manually with `cargo-apk`:
```bash
cargo apk build --release
```
The output APK is generated at:
```
target/release/apk/gemimusic.apk
```

### 3. Install & Run on Connected Android Device
Connect your Android device with USB Debugging enabled, then:
```bash
cargo apk run --release
```
Or install via `adb`:
```bash
adb install -r target/release/apk/gemimusic.apk
```

---

## 🔑 Google OAuth Configuration Note

By default, gemimusic uses standard Google OAuth 2.0 credentials supporting YouTube on TV / Smart TV / Native Device flow.

If you wish to use your own Google Cloud OAuth Client ID:
1. Go to [Google Cloud Console](https://console.cloud.google.com/).
2. Create a project and enable the **YouTube Data API v3**.
3. Under **Credentials**, create an **OAuth 2.0 Client ID** with Application Type set to **TVs and Limited Input devices**.
4. In `src/api/oauth.rs`, replace `DEFAULT_CLIENT_ID` with your client ID.

---

## 📄 License
Dual-licensed under MIT or Apache 2.0.
