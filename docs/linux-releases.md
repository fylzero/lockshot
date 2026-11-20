# Linux releases

The Release workflow's `linux` job builds the Linux packages after the `macos` job
succeeds and uploads them to the same GitHub release. It needs no secrets beyond the
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` the macOS job
already uses. Linux has no code signing or notarization step.

## What gets published

The job runs once on `ubuntu-22.04` (x64) and once on `ubuntu-22.04-arm` (ARM64).
Building on 22.04 keeps the binaries working on distros with glibc 2.35 or newer.
Besides the usual Tauri packages, the job installs the PipeWire, XCB, and GBM
development libraries that screen capture needs on Wayland and X11.

| Asset | Contents |
| --- | --- |
| `Lockshot-linux-x64.AppImage`, `Lockshot-linux-arm64.AppImage` | Self-contained app for any distro |
| `Lockshot-linux-x64.deb`, `Lockshot-linux-arm64.deb` | Debian and Ubuntu package |
| `Lockshot-linux-x64.rpm`, `Lockshot-linux-arm64.rpm` | Fedora, RHEL, and openSUSE package |
| `*.sig` | Updater signatures for each package |

The names stay the same from release to release, so the README links to
`releases/latest/download/…` keep working.

## Updates

Each architecture adds `linux-<arch>-appimage`, `linux-<arch>-deb`, and
`linux-<arch>-rpm` entries to the release's `latest.json`, and the updater picks the
one matching how Lockshot was installed. The AppImage replaces itself in place. The
`.deb` and `.rpm` are installed with `pkexec`, so the user is asked for their
password.

The jobs run one after another (`needs: macos` and `max-parallel: 1`) because each
one downloads `latest.json`, adds its entries, and uploads it again. Running them at
the same time would let one job overwrite the entries another just added.

If the `macos` job fails, the Linux job doesn't run. Re-run the failed jobs from the
Actions page once the problem is fixed.

## Verification

After the first Linux release, install the `.deb` on Ubuntu (GNOME on Wayland) and
the AppImage on an X11 desktop such as KDE on X11, then confirm:

- The tray icon appears (GNOME needs the AppIndicator extension).
- On X11, the capture hotkeys work from another app.
- On Wayland, `lockshot --area` and `lockshot --screen` bound to keys in the
  desktop's settings start a capture, after approving the screenshot prompt.
- Saving and **Show in File Manager** work.
- **Check for Updates…** in the tray menu reports that Lockshot is up to date.
