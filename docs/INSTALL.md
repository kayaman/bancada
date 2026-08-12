# Installing Bancada

Bancada ships as three Linux installables, all produced by one build:

| Package | For | File |
|---------|-----|------|
| RPM | Fedora, openSUSE, RHEL-family | `Bancada-<version>-1.x86_64.rpm` |
| DEB | Debian, Ubuntu, Mint | `Bancada_<version>_amd64.deb` |
| AppImage | any distro, no root needed | `Bancada_<version>_amd64.AppImage` |

There is no hosted download yet — build the bundles from a checkout
(section 1), then install the one matching your distro (section 2).

## 1. Build the installables

Prerequisites are the same as for development — Rust, Node 20+, and the
Tauri system libraries (webkit2gtk 4.1, openssl, librsvg; see the
README's *Prerequisites* section for your distro's package names). Then:

```bash
npm install
npm run tauri build
```

If the AppImage step fails with `failed to run linuxdeploy`, one or
both of these apply (both hit stock Fedora):

- **No `libfuse.so.2`** (package `fuse-libs` on Fedora): linuxdeploy is
  itself an AppImage and needs FUSE to run. Install the package, or set
  `APPIMAGE_EXTRACT_AND_RUN=1` to have it self-extract instead.
- **`strip` errors about `.relr.dyn`**: linuxdeploy's bundled binutils
  predates RELR relocations used by modern distro libraries. Set
  `NO_STRIP=true` (the bundled libs stay unstripped).

```bash
NO_STRIP=true APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri build
```

The bundles land in:

```
target/release/bundle/rpm/Bancada-<version>-1.x86_64.rpm
target/release/bundle/deb/Bancada_<version>_amd64.deb
target/release/bundle/appimage/Bancada_<version>_amd64.AppImage
```

## 2. Install the app

**Fedora / openSUSE (RPM):**

```bash
sudo dnf install ./Bancada-<version>-1.x86_64.rpm     # Fedora
sudo zypper install ./Bancada-<version>-1.x86_64.rpm  # openSUSE
```

**Debian / Ubuntu (DEB):**

```bash
sudo apt install ./Bancada_<version>_amd64.deb
```

**AppImage (no root):**

```bash
chmod +x Bancada_<version>_amd64.AppImage
./Bancada_<version>_amd64.AppImage
```

AppImages need `libfuse.so.2` at runtime (package `fuse-libs` on
Fedora, `libfuse2` on Ubuntu). Without it, run
`./Bancada_<version>_amd64.AppImage --appimage-extract-and-run`.

RPM and DEB install `bancada` on the PATH plus a desktop entry
(Development category) with the full icon set
(32/128/256/512 px hicolor), so Bancada appears in your app menu/grid
with its icon. The AppImage is self-contained; if you want a menu entry
for it, run it through an AppImage integrator (e.g. Gear Lever or
AppImageLauncher).

**Upgrade:** install the newer package the same way — the package
managers treat it as an update. **Uninstall:** `sudo dnf remove bancada`
/ `sudo zypper remove bancada` / `sudo apt remove bancada`, or just
delete the AppImage.

## 3. Install the engines Bancada drives

Bancada does not bundle a toolchain; it resolves these from your PATH:

| Tool | Needed for | Install |
|------|-----------|---------|
| `arduino-cli` | everything — boards, builds, uploads, libraries (**required**) | `curl -fsSL https://raw.githubusercontent.com/arduino/arduino-cli/master/install.sh \| sh` |
| `esptool` | ESP utilities (read MAC, chip info) | `pip install --user esptool` |
| `git` (≥ 2.25) | fetching pinned libraries from repos | your distro's package |
| `claude` | the AI Assistant panel | see <https://claude.com/product/claude-code>; run `claude` once to sign in |

Only `arduino-cli` is a hard requirement; the others unlock their
features when present. After installing `arduino-cli`, add at least one
core (Bancada's board pickers list what the CLI knows):

```bash
arduino-cli core update-index
arduino-cli core install esp32:esp32     # or arduino:avr, etc.
```

## 4. Serial-port access (one-time)

Your user needs to be in the group that owns `/dev/ttyACM*` /
`/dev/ttyUSB*`:

```bash
ls -l /dev/ttyACM0              # shows the owning group
sudo usermod -aG dialout $USER  # dialout on Fedora/openSUSE/Debian; uucp on Arch
```

Log out and back in for the group to apply.

**ModemManager grabs bench ports.** On stock desktop installs,
ModemManager probes every newly-plugged serial device with a ~4 s
exclusive AT-command probe — during which flashing and the serial
monitor fail with a busy port. Its `STRICT` filter policy does **not**
exempt CDC-ACM bridges, so the reliable fix is a udev ignore rule for
the common Arduino/ESP USB bridges:

```bash
sudo tee /etc/udev/rules.d/99-bancada-serial.rules >/dev/null <<'EOF'
# Keep ModemManager off Arduino/ESP dev-board serial bridges
ATTRS{idVendor}=="303a", ENV{ID_MM_DEVICE_IGNORE}="1"  # Espressif native USB
ATTRS{idVendor}=="10c4", ENV{ID_MM_DEVICE_IGNORE}="1"  # Silicon Labs CP210x
ATTRS{idVendor}=="1a86", ENV{ID_MM_DEVICE_IGNORE}="1"  # WCH CH340/CH341
ATTRS{idVendor}=="0403", ENV{ID_MM_DEVICE_IGNORE}="1"  # FTDI
ATTRS{idVendor}=="2341", ENV{ID_MM_DEVICE_IGNORE}="1"  # Arduino SA
EOF
sudo udevadm control --reload-rules && sudo udevadm trigger
```

## 5. First run

Launch **Bancada** from the app menu (or `bancada` in a terminal). From the
📁 project menu, open a project or create a new one, pick a board, and
Verify. If
the board picker is empty, install a core (section 3); if flashing says
the port is busy, revisit section 4.
