# Linux bundle metadata

This directory holds Linux-specific packaging assets for Turnstile. The
bundle configuration lives under `bundle.linux` in
[`../tauri.conf.json`](../tauri.conf.json).

The build itself runs on Ubuntu, so only the **declared** runtime
dependencies matter for installs on other distributions — the bundler does
not resolve them against the build host.

## `.desktop` entry

`turnstile.desktop` is a [Handlebars] template referenced by both
`deb.desktopTemplate` and `rpm.desktopTemplate`. The bundler fills the
`{{categories}}`, `{{comment}}`, `{{exec}}`, `{{icon}}`, `{{name}}` and
`{{mime_type}}` variables; `categories` comes from `bundle.category`
(`Education`) and `comment` from `bundle.shortDescription`. `Keywords` is
hard-coded in the template because the bundler exposes no variable for it.

The application icon and `Name` come from the standard `bundle.icon` array
and `productName`, so the app shows up in the desktop menu with the right
icon once installed.

[Handlebars]: https://handlebarsjs.com/

## Runtime dependencies across distributions

The webkit2gtk runtime and the app-indicator library are named differently
on Debian/Ubuntu vs. the RPM distributions, so the `deb` and `rpm` `depends`
lists are maintained separately.

| Runtime | Debian / Ubuntu (`deb`)        | Fedora / RHEL (`rpm`)      | openSUSE (rpm)                  |
| ------- | ------------------------------ | -------------------------- | ------------------------------- |
| WebKit  | `libwebkit2gtk-4.1-0`          | `webkit2gtk4.1`            | `libwebkit2gtk-4_1-0`           |
| GTK 3   | `libgtk-3-0`                   | `gtk3`                     | `gtk3`                          |
| Tray    | `libayatana-appindicator3-1`   | `libappindicator-gtk3`     | `libayatana-appindicator3-1`    |
| SVG     | (pulled in via GTK)            | `librsvg2`                 | `librsvg-2-2`                   |

The declared `rpm` `depends` use **Fedora/RHEL** package names. openSUSE
uses different names (`libwebkit2gtk-4_1-0`, `librsvg-2-2`,
`libayatana-appindicator3-1`) that the Fedora-named dependencies will not
satisfy, so the `.rpm` is not guaranteed to resolve dependencies on
openSUSE. The **AppImage is the fallback** for any distribution whose
package names we do not (or cannot) match: it bundles the webkit2gtk
runtime, and `appimage.bundleMediaFramework` is enabled so GStreamer media
codecs travel with it for distros that ship neither.

## Acceptance targets

- `.deb` installs on Ubuntu 22.04+ and Debian 12 via
  `apt install ./Turnstile.deb` (pulls the declared `depends`).
- `.rpm` installs on current Fedora via `dnf install ./Turnstile.rpm`.
- `.AppImage` launches on a distro without webkit2gtk preinstalled.
