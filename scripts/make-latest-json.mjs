#!/usr/bin/env node
//
// Build the Tauri v2 updater manifest (`latest.json`) from the signed update
// bundles staged in a directory. See issues #50 and #52.
//
// The release pipeline runs `tauri build` with `bundle.createUpdaterArtifacts`
// enabled, which emits a per-platform update bundle plus a detached minisign
// signature (`<bundle>.sig`) for each:
//
//     macOS (universal):  Turnstile.app.tar.gz                    + .sig
//     Linux (AppImage):   Turnstile_<ver>_amd64.AppImage.tar.gz   + .sig
//
// This script discovers those bundles by their `.sig` siblings — so it is
// agnostic to Tauri's versioned filenames — reads each signature, and writes a
// `latest.json` whose platform URLs point at the matching Release assets
// (`releases/download/<tag>/<bundle>`). The in-app updater plugin (#53) fetches
// this manifest from `releases/latest/download/latest.json`, compares the
// `version`, and verifies the downloaded bundle against the embedded public key.
//
// A universal macOS bundle serves both `darwin-x86_64` and `darwin-aarch64`, so
// a single `.app.tar.gz` is mapped to both keys. Linux update bundles are
// AppImage-only (deb/rpm are owned by the system package manager and can't
// self-update — see #50), so the manifest carries no deb/rpm entries.
//
// Usage:
//   node scripts/make-latest-json.mjs --dir DIR --version V --tag TAG \
//     [--repo owner/name] [--out FILE] [--notes TEXT] [--pub-date ISO]
//
//   --dir DIR        Directory holding the signed update bundles and their
//                    `.sig` files (the assembled release `dist/`).
//   --version V      The release version (e.g. 1.0.0), from package.json.
//   --tag TAG        The Release tag the assets live under (e.g. v1.0.0).
//   --repo owner/name  GitHub repo for the asset URLs. Default: wpm/Turnstile.
//   --out FILE       Where to write the manifest. Default: DIR/latest.json.
//   --notes TEXT     Release notes string. Default: "".
//   --pub-date ISO   RFC 3339 publish date. Default: now.
//
// Exits non-zero on any ambiguity (two bundles for one platform), an
// unrecognized signed bundle, or when no update bundles are found — so a
// silently broken build fails the release rather than shipping a partial or
// empty manifest.

import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const DEFAULT_REPO = "wpm/Turnstile";

/** Map a signed update bundle's filename to the manifest platform key(s). */
function platformsForBundle(bundle) {
  if (bundle.endsWith(".app.tar.gz")) {
    // A universal macOS bundle updates both Intel and Apple Silicon installs.
    return ["darwin-x86_64", "darwin-aarch64"];
  }
  if (bundle.endsWith(".AppImage.tar.gz")) {
    return ["linux-x86_64"];
  }
  // Windows is not built today, but recognize its updater bundles so a future
  // matrix addition flows through without a silent drop.
  if (bundle.endsWith(".nsis.zip") || bundle.endsWith(".msi.zip")) {
    return ["windows-x86_64"];
  }
  return null;
}

function die(message) {
  process.stderr.write(`make-latest-json: ${message}\n`);
  process.exit(1);
}

function parseArgs(argv) {
  const args = {
    repo: DEFAULT_REPO,
    notes: "",
    pubDate: new Date().toISOString(),
  };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const need = () => {
      const v = argv[++i];
      if (v === undefined) die(`${flag} requires a value`);
      return v;
    };
    switch (flag) {
      case "--dir":
        args.dir = need();
        break;
      case "--version":
        args.version = need();
        break;
      case "--tag":
        args.tag = need();
        break;
      case "--repo":
        args.repo = need();
        break;
      case "--out":
        args.out = need();
        break;
      case "--notes":
        args.notes = need();
        break;
      case "--pub-date":
        args.pubDate = need();
        break;
      default:
        die(`unknown argument: ${flag}`);
    }
  }
  if (!args.dir) die("--dir DIR is required");
  if (!args.version) die("--version V is required");
  if (!args.tag) die("--tag TAG is required");
  args.out ??= join(args.dir, "latest.json");
  return args;
}

function buildManifest(args) {
  const sigs = readdirSync(args.dir)
    .filter((name) => name.endsWith(".sig"))
    .sort();

  const platforms = {};
  for (const sig of sigs) {
    const bundle = sig.slice(0, -".sig".length);
    const keys = platformsForBundle(bundle);
    if (keys === null) {
      die(`unrecognized signed update bundle: ${bundle}`);
    }
    const signature = readFileSync(join(args.dir, sig), "utf8").trim();
    const url = `https://github.com/${args.repo}/releases/download/${args.tag}/${encodeURIComponent(bundle)}`;
    for (const key of keys) {
      if (platforms[key]) {
        die(`two update bundles map to platform '${key}'`);
      }
      platforms[key] = { signature, url };
    }
  }

  if (Object.keys(platforms).length === 0) {
    die(`no signed update bundles (*.sig) found in ${args.dir}`);
  }

  return {
    version: args.version,
    notes: args.notes,
    pub_date: args.pubDate,
    platforms,
  };
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const manifest = buildManifest(args);
  writeFileSync(args.out, `${JSON.stringify(manifest, null, 2)}\n`);
  process.stdout.write(`Wrote ${args.out}:\n`);
  for (const key of Object.keys(manifest.platforms)) {
    process.stdout.write(`  ${key} -> ${manifest.platforms[key].url}\n`);
  }
}

main();
