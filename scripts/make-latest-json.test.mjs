/**
 * Tests for scripts/make-latest-json.mjs (issues #50, #52): the Tauri v2
 * updater manifest is assembled from the signed update bundles staged in a
 * directory, discovered by their `.sig` siblings.
 *
 * Run with `pnpm test:scripts`. These shell out to `node` so they exercise the
 * script exactly as the release `attach` job does.
 */
import { execFileSync } from "node:child_process";
import {
  mkdtempSync,
  writeFileSync,
  readFileSync,
  existsSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const SCRIPT = join(
  dirname(fileURLToPath(import.meta.url)),
  "make-latest-json.mjs",
);

let work;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), "make-latest-json-"));
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
});

/** Run the script, returning {status, stdout, stderr}; never throws on failure. */
function run(...args) {
  try {
    const stdout = execFileSync("node", [SCRIPT, ...args], {
      encoding: "utf8",
    });
    return { status: 0, stdout, stderr: "" };
  } catch (err) {
    return {
      status: err.status ?? 1,
      stdout: err.stdout ?? "",
      stderr: err.stderr ?? "",
    };
  }
}

/** Write a fake update bundle and its detached signature into `work`. */
function stageBundle(name, signature) {
  writeFileSync(join(work, name), `bundle bytes of ${name}\n`);
  writeFileSync(join(work, `${name}.sig`), `${signature}\n`);
}

function readManifest() {
  return JSON.parse(readFileSync(join(work, "latest.json"), "utf8"));
}

describe("make-latest-json.mjs", () => {
  it("maps a universal macOS bundle to both darwin keys with one URL", () => {
    stageBundle("Turnstile.app.tar.gz", "MAC_SIG");

    const { status, stderr } = run(
      "--dir",
      work,
      "--version",
      "1.2.0",
      "--tag",
      "v1.2.0",
    );
    expect(stderr).toBe("");
    expect(status).toBe(0);

    const m = readManifest();
    expect(m.version).toBe("1.2.0");
    expect(Object.keys(m.platforms).sort()).toEqual([
      "darwin-aarch64",
      "darwin-x86_64",
    ]);
    expect(m.platforms["darwin-x86_64"].signature).toBe("MAC_SIG");
    expect(m.platforms["darwin-aarch64"].signature).toBe("MAC_SIG");
    const url =
      "https://github.com/wpm/Turnstile/releases/download/v1.2.0/Turnstile.app.tar.gz";
    expect(m.platforms["darwin-x86_64"].url).toBe(url);
    expect(m.platforms["darwin-aarch64"].url).toBe(url);
  });

  it("maps an AppImage bundle to linux-x86_64 with the versioned asset URL", () => {
    stageBundle("Turnstile_1.2.0_amd64.AppImage.tar.gz", "LINUX_SIG");

    const { status } = run(
      "--dir",
      work,
      "--version",
      "1.2.0",
      "--tag",
      "v1.2.0",
    );
    expect(status).toBe(0);

    const m = readManifest();
    expect(Object.keys(m.platforms)).toEqual(["linux-x86_64"]);
    expect(m.platforms["linux-x86_64"].signature).toBe("LINUX_SIG");
    expect(m.platforms["linux-x86_64"].url).toBe(
      "https://github.com/wpm/Turnstile/releases/download/v1.2.0/Turnstile_1.2.0_amd64.AppImage.tar.gz",
    );
  });

  it("assembles a full mac + linux manifest from a merged dist dir", () => {
    stageBundle("Turnstile.app.tar.gz", "MAC_SIG");
    stageBundle("Turnstile_1.0.0_amd64.AppImage.tar.gz", "LINUX_SIG");

    const { status } = run(
      "--dir",
      work,
      "--version",
      "1.0.0",
      "--tag",
      "v1.0.0",
      "--notes",
      "Release notes here",
      "--pub-date",
      "2026-01-02T03:04:05Z",
    );
    expect(status).toBe(0);

    const m = readManifest();
    expect(m.notes).toBe("Release notes here");
    expect(m.pub_date).toBe("2026-01-02T03:04:05Z");
    expect(Object.keys(m.platforms).sort()).toEqual([
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-x86_64",
    ]);
  });

  it("honors --repo and --out", () => {
    stageBundle("Turnstile.app.tar.gz", "MAC_SIG");
    const out = join(work, "nested", "manifest.json");
    // --out's parent must exist; the release job writes into the dist dir.
    execFileSync("mkdir", ["-p", dirname(out)]);

    const { status } = run(
      "--dir",
      work,
      "--version",
      "9.9.9",
      "--tag",
      "v9.9.9",
      "--repo",
      "acme/Widget",
      "--out",
      out,
    );
    expect(status).toBe(0);
    expect(existsSync(out)).toBe(true);
    const m = JSON.parse(readFileSync(out, "utf8"));
    expect(m.platforms["darwin-x86_64"].url).toContain(
      "https://github.com/acme/Widget/releases/download/v9.9.9/",
    );
  });

  it("fills pub_date with a valid timestamp when not given", () => {
    stageBundle("Turnstile.app.tar.gz", "MAC_SIG");
    run("--dir", work, "--version", "1.0.0", "--tag", "v1.0.0");
    const m = readManifest();
    expect(Number.isNaN(Date.parse(m.pub_date))).toBe(false);
  });

  it("fails when the directory holds no signed update bundles", () => {
    writeFileSync(join(work, "SHA256SUMS"), "deadbeef  Turnstile.dmg\n");
    const { status, stderr } = run(
      "--dir",
      work,
      "--version",
      "1.0.0",
      "--tag",
      "v1.0.0",
    );
    expect(status).not.toBe(0);
    expect(stderr).toMatch(/no signed update bundles/);
  });

  it("fails on an unrecognized signed bundle rather than dropping it", () => {
    stageBundle("Turnstile_1.0.0.weird.bundle", "MYSTERY_SIG");
    const { status, stderr } = run(
      "--dir",
      work,
      "--version",
      "1.0.0",
      "--tag",
      "v1.0.0",
    );
    expect(status).not.toBe(0);
    expect(stderr).toMatch(/unrecognized signed update bundle/);
  });

  it("fails when two bundles map to the same platform", () => {
    stageBundle("Turnstile_1.0.0_amd64.AppImage.tar.gz", "SIG_A");
    stageBundle("Turnstile_1.0.1_amd64.AppImage.tar.gz", "SIG_B");
    const { status, stderr } = run(
      "--dir",
      work,
      "--version",
      "1.0.0",
      "--tag",
      "v1.0.0",
    );
    expect(status).not.toBe(0);
    expect(stderr).toMatch(/two update bundles map to platform/);
  });

  it("requires --dir, --version and --tag", () => {
    expect(run("--version", "1.0.0", "--tag", "v1.0.0").stderr).toMatch(
      /--dir/,
    );
    expect(run("--dir", work, "--tag", "v1.0.0").stderr).toMatch(/--version/);
    expect(run("--dir", work, "--version", "1.0.0").stderr).toMatch(/--tag/);
  });
});
