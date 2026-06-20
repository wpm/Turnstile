import { describe, expect, it, vi } from "vitest";
import {
  runUpdateFlow,
  type Update,
  type UpdaterDeps,
  type ToastSeverity,
} from "./updater";

interface FakeUpdate extends Update {
  installs: number;
}

/** A fake `Update` whose install can be made to succeed or reject. */
function fakeUpdate(
  overrides: {
    version?: string;
    currentVersion?: string;
    installError?: Error;
  } = {},
): FakeUpdate {
  const update: FakeUpdate = {
    version: overrides.version ?? "1.2.0",
    currentVersion: overrides.currentVersion ?? "1.1.0",
    installs: 0,
    downloadAndInstall: vi.fn((): Promise<void> => {
      update.installs += 1;
      return overrides.installError
        ? Promise.reject(overrides.installError)
        : Promise.resolve();
    }),
  };
  return update;
}

interface Toast {
  severity: ToastSeverity;
  message: string;
}

/**
 * Build deps with sensible defaults (supported install, an update offered, the
 * user accepting) plus a recording `notify`. Override per test.
 */
function makeDeps(over: Partial<UpdaterDeps> = {}): {
  deps: UpdaterDeps;
  toasts: Toast[];
  relaunch: ReturnType<typeof vi.fn>;
} {
  const toasts: Toast[] = [];
  const relaunch = vi.fn((): Promise<void> => Promise.resolve());
  const deps: UpdaterDeps = {
    isSupported: over.isSupported ?? (() => Promise.resolve(true)),
    check: over.check ?? (() => Promise.resolve(fakeUpdate())),
    confirm: over.confirm ?? (() => Promise.resolve(true)),
    relaunch: over.relaunch ?? relaunch,
    notify:
      over.notify ??
      ((severity, message) => {
        toasts.push({ severity, message });
      }),
    log: over.log,
  };
  return { deps, toasts, relaunch };
}

describe("runUpdateFlow", () => {
  it("installs and relaunches when an update is offered and accepted", async () => {
    const update = fakeUpdate({ version: "2.0.0" });
    const { deps, toasts, relaunch } = makeDeps({
      check: () => Promise.resolve(update),
    });

    await runUpdateFlow(deps);

    expect(update.installs).toBe(1);
    expect(relaunch).toHaveBeenCalledOnce();
    // The user is told the download is happening — one info toast, no errors.
    expect(toasts).toHaveLength(1);
    expect(toasts[0].severity).toBe("info");
    expect(toasts[0].message).toContain("Downloading");
  });

  it("does nothing when the install type can't self-update (deb/rpm)", async () => {
    const check = vi.fn(
      (): Promise<Update | null> => Promise.resolve(fakeUpdate()),
    );
    const { deps, relaunch } = makeDeps({
      isSupported: () => Promise.resolve(false),
      check,
    });

    await runUpdateFlow(deps);

    // Never even checks for an update on an unsupported install.
    expect(check).not.toHaveBeenCalled();
    expect(relaunch).not.toHaveBeenCalled();
  });

  it("stays silent when already up to date", async () => {
    const { deps, toasts, relaunch } = makeDeps({
      check: () => Promise.resolve(null),
    });

    await runUpdateFlow(deps);

    expect(toasts).toEqual([]);
    expect(relaunch).not.toHaveBeenCalled();
  });

  it("does not install when the user declines", async () => {
    const update = fakeUpdate();
    const { deps, toasts, relaunch } = makeDeps({
      check: () => Promise.resolve(update),
      confirm: () => Promise.resolve(false),
    });

    await runUpdateFlow(deps);

    expect(update.installs).toBe(0);
    expect(relaunch).not.toHaveBeenCalled();
    expect(toasts).toEqual([]);
  });

  it("swallows a failed check without toasting (offline / prerelease only)", async () => {
    const log = vi.fn();
    const { deps, toasts, relaunch } = makeDeps({
      check: () => Promise.reject(new Error("network down")),
      log,
    });

    await expect(runUpdateFlow(deps)).resolves.toBeUndefined();

    expect(toasts).toEqual([]);
    expect(relaunch).not.toHaveBeenCalled();
    expect(log).toHaveBeenCalledWith(expect.stringContaining("network down"));
  });

  it("surfaces an error toast when the download/install fails", async () => {
    const update = fakeUpdate({ installError: new Error("disk full") });
    const { deps, toasts, relaunch } = makeDeps({
      check: () => Promise.resolve(update),
    });

    await runUpdateFlow(deps);

    expect(relaunch).not.toHaveBeenCalled();
    const errorToast = toasts.find((t) => t.severity === "error");
    expect(errorToast?.message).toContain("disk full");
  });

  it("tells the user to restart when install succeeds but relaunch fails", async () => {
    const update = fakeUpdate();
    const { deps, toasts } = makeDeps({
      check: () => Promise.resolve(update),
      relaunch: () => Promise.reject(new Error("relaunch blocked")),
    });

    await runUpdateFlow(deps);

    expect(update.installs).toBe(1);
    const restartToast = toasts.find((t) => t.message.includes("Restart"));
    expect(restartToast?.severity).toBe("info");
  });

  it("does not throw when the support check itself rejects", async () => {
    const { deps, relaunch } = makeDeps({
      isSupported: () => Promise.reject(new Error("no backend")),
    });

    await expect(runUpdateFlow(deps)).resolves.toBeUndefined();
    expect(relaunch).not.toHaveBeenCalled();
  });
});
