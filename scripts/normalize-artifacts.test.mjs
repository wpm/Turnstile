/**
 * Tests for scripts/normalize-artifacts.sh (issue #24): Tauri's
 * version-embedded bundle names are copied to stable, version-less names and a
 * SHA256SUMS manifest is written over them.
 *
 * Run with `pnpm test:scripts`. These shell out to the script, so they need a
 * POSIX environment with bash and a sha256 tool (sha256sum or shasum) — both
 * present on the CI runners and on the macOS/Linux release legs the script
 * targets.
 */
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdtempSync,
  mkdirSync,
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
  "normalize-artifacts.sh",
);

/** The version-less names the website deep-links to, keyed by bundle type. */
const STABLE = {
  dmg: "Turnstile-macOS-universal.dmg",
  deb: "Turnstile-linux-x86_64.deb",
  rpm: "Turnstile-linux-x86_64.rpm",
  AppImage: "Turnstile-linux-x86_64.AppImage",
};

let work;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), "normalize-artifacts-"));
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
});

/** Run the script, returning {status, stdout, stderr}; never throws on failure. */
function run(...args) {
  try {
    const stdout = execFileSync("bash", [SCRIPT, ...args], {
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

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

/**
 * Lay out a fake Tauri bundle tree under `dir`, mirroring the real
 * `target/release/bundle/<type>/<versioned-name>` layout, with unique content
 * per file so checksum collisions can't hide a mix-up.
 */
function makeBundle(dir, files) {
  for (const [subdir, name] of files) {
    const d = join(dir, subdir);
    mkdirSync(d, { recursive: true });
    writeFileSync(join(d, name), `contents of ${name}\n`);
  }
}

describe("normalize-artifacts.sh", () => {
  it("copies every Tauri bundle to its stable name preserving contents", () => {
    const inDir = join(work, "bundle");
    const outDir = join(work, "dist");
    makeBundle(inDir, [
      ["dmg", "Turnstile_1.0.0_universal.dmg"],
      ["deb", "Turnstile_1.0.0_amd64.deb"],
      ["rpm", "Turnstile-1.0.0-1.x86_64.rpm"],
      ["appimage", "Turnstile_1.0.0_amd64.AppImage"],
    ]);

    const { status, stderr } = run("--in", inDir, "--out", outDir);
    expect(stderr).toBe("");
    expect(status).toBe(0);

    for (const stable of Object.values(STABLE)) {
      expect(existsSync(join(outDir, stable)), `${stable} should exist`).toBe(
        true,
      );
    }
    // Each stable copy carries the bytes of the versioned original it came from.
    expect(readFileSync(join(outDir, STABLE.dmg), "utf8")).toContain(
      "universal.dmg",
    );
    expect(readFileSync(join(outDir, STABLE.deb), "utf8")).toContain(
      "amd64.deb",
    );
    expect(readFileSync(join(outDir, STABLE.rpm), "utf8")).toContain(
      "x86_64.rpm",
    );
    expect(readFileSync(join(outDir, STABLE.AppImage), "utf8")).toContain(
      "amd64.AppImage",
    );
  });

  it("leaves the versioned originals in place (copy, not move)", () => {
    const inDir = join(work, "bundle");
    makeBundle(inDir, [["dmg", "Turnstile_1.0.0_universal.dmg"]]);
    run("--in", inDir, "--out", join(work, "dist"));
    expect(
      existsSync(join(inDir, "dmg", "Turnstile_1.0.0_universal.dmg")),
    ).toBe(true);
  });

  it("normalizes only the types present (platform legs build disjoint sets)", () => {
    const inDir = join(work, "bundle");
    const outDir = join(work, "dist");
    // A macOS leg: only the dmg is present.
    makeBundle(inDir, [["dmg", "Turnstile_1.0.0_universal.dmg"]]);
    const { status } = run("--in", inDir, "--out", outDir);
    expect(status).toBe(0);
    expect(existsSync(join(outDir, STABLE.dmg))).toBe(true);
    expect(existsSync(join(outDir, STABLE.deb))).toBe(false);
  });

  it("writes a SHA256SUMS that matches the normalized files", () => {
    const inDir = join(work, "bundle");
    const outDir = join(work, "dist");
    makeBundle(inDir, [
      ["dmg", "Turnstile_1.0.0_universal.dmg"],
      ["deb", "Turnstile_1.0.0_amd64.deb"],
      ["rpm", "Turnstile-1.0.0-1.x86_64.rpm"],
      ["appimage", "Turnstile_1.0.0_amd64.AppImage"],
    ]);

    const { status } = run("--in", inDir, "--out", outDir, "--checksums");
    expect(status).toBe(0);

    const manifest = readFileSync(join(outDir, "SHA256SUMS"), "utf8").trimEnd();
    const lines = manifest.split("\n");
    expect(lines).toHaveLength(4);

    // The manifest must not checksum itself.
    expect(manifest).not.toContain("SHA256SUMS");

    for (const line of lines) {
      const m = line.match(/^([0-9a-f]{64}) {2}(.+)$/);
      expect(m, `well-formed line: ${line}`).not.toBeNull();
      const [, hash, name] = m;
      // Listed by basename, and the hash matches the actual file's bytes.
      expect(Object.values(STABLE)).toContain(name);
      expect(hash).toBe(sha256(join(outDir, name)));
    }
  });

  it("emits a deterministic, C-sorted manifest", () => {
    const inDir = join(work, "bundle");
    const outDir = join(work, "dist");
    makeBundle(inDir, [
      ["dmg", "Turnstile_1.0.0_universal.dmg"],
      ["deb", "Turnstile_1.0.0_amd64.deb"],
      ["rpm", "Turnstile-1.0.0-1.x86_64.rpm"],
      ["appimage", "Turnstile_1.0.0_amd64.AppImage"],
    ]);
    run("--in", inDir, "--out", outDir, "--checksums");
    const names = readFileSync(join(outDir, "SHA256SUMS"), "utf8")
      .trimEnd()
      .split("\n")
      .map((l) => l.replace(/^[0-9a-f]{64} {2}/, ""));
    expect(names).toEqual([...names].sort());
  });

  it("can checksum a pre-assembled directory with --checksums alone", () => {
    const outDir = join(work, "dist");
    mkdirSync(outDir);
    writeFileSync(join(outDir, STABLE.dmg), "dmg\n");
    writeFileSync(join(outDir, STABLE.deb), "deb\n");

    const { status } = run("--out", outDir, "--checksums");
    expect(status).toBe(0);
    const lines = readFileSync(join(outDir, "SHA256SUMS"), "utf8")
      .trimEnd()
      .split("\n");
    expect(lines).toHaveLength(2);
  });

  it("fails when two artifacts of the same type are present", () => {
    const inDir = join(work, "bundle");
    makeBundle(inDir, [
      ["dmg", "Turnstile_1.0.0_universal.dmg"],
      ["dmg", "Turnstile_0.9.0_universal.dmg"],
    ]);
    const { status, stderr } = run("--in", inDir, "--out", join(work, "dist"));
    expect(status).not.toBe(0);
    expect(stderr).toMatch(/ambiguous/);
  });

  it("fails when --in has no recognized artifacts", () => {
    const inDir = join(work, "bundle");
    mkdirSync(inDir, { recursive: true });
    writeFileSync(join(inDir, "notes.txt"), "nothing to bundle\n");
    const { status, stderr } = run("--in", inDir, "--out", join(work, "dist"));
    expect(status).not.toBe(0);
    expect(stderr).toMatch(/no .* artifacts found/);
  });

  it("requires --out", () => {
    const { status, stderr } = run("--checksums");
    expect(status).not.toBe(0);
    expect(stderr).toMatch(/--out/);
  });

  it("requires at least one of --in or --checksums", () => {
    const { status, stderr } = run("--out", join(work, "dist"));
    expect(status).not.toBe(0);
    expect(stderr).toMatch(/nothing to do/);
  });
});
