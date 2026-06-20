// In-app auto-update flow (#53).
//
// Turnstile checks for updates once on launch, non-blocking: if a newer signed
// release is published it asks the user, downloads + installs the verified
// bundle, then relaunches. The mechanics — fetching the signed `latest.json`
// (#52), verifying it against the embedded public key, and swapping the
// install — live in the Tauri updater plugin; this module is the orchestration
// and user-facing policy around it.
//
// The plugin bindings are injected (see `UpdaterDeps`) rather than imported
// here so the flow is unit-testable without a Tauri runtime, and so the page
// can lazy-load the native plugins only when actually running under Tauri.

/** A download lifecycle event from the updater plugin's `downloadAndInstall`. */
export interface DownloadEvent {
  event: "Started" | "Progress" | "Finished";
  data?: { contentLength?: number; chunkLength?: number };
}

/** The subset of the plugin's `Update` object this flow uses. */
export interface Update {
  /** Version offered by the manifest, e.g. "1.2.0". */
  version: string;
  /** Version currently installed. */
  currentVersion: string;
  /** Download the verified bundle and install it, reporting progress. */
  downloadAndInstall(onEvent?: (event: DownloadEvent) => void): Promise<void>;
}

export type ToastSeverity = "info" | "error";

/** Injected bindings for the update flow — the seam the tests drive. */
export interface UpdaterDeps {
  /**
   * Whether this install type can self-update. deb/rpm installs are owned by
   * the system package manager and can't (see #50); the backend
   * `updater_target_supported` command answers this.
   */
  isSupported: () => Promise<boolean>;
  /**
   * Fetch + verify the manifest. Resolves to an `Update` when a newer version
   * is available, or `null` when already up to date.
   */
  check: () => Promise<Update | null>;
  /** Ask the user whether to install now; resolves true to proceed. */
  confirm: (message: string, title: string) => Promise<boolean>;
  /** Quit and relaunch into the freshly installed version. */
  relaunch: () => Promise<void>;
  /** Surface a short message to the user (a toast). */
  notify: (severity: ToastSeverity, message: string) => void;
  /** Record diagnostics that shouldn't bother the user (e.g. a failed check). */
  log?: (message: string) => void;
}

/**
 * Run the launch-time update check. Designed to never throw and never block
 * startup: any failure to *check* (offline, an unsupported install type, a
 * malformed manifest) resolves quietly to "no update right now". Only a failure
 * that occurs after the user has agreed to update is surfaced as a toast.
 */
export async function runUpdateFlow(deps: UpdaterDeps): Promise<void> {
  let supported: boolean;
  try {
    supported = await deps.isSupported();
  } catch (e) {
    deps.log?.(`updater support check failed: ${String(e)}`);
    return;
  }
  if (!supported) {
    deps.log?.("updater not supported on this install type; skipping");
    return;
  }

  let update: Update | null;
  try {
    update = await deps.check();
  } catch (e) {
    // A failed check is expected and benign (no network, prerelease-only, etc.)
    // — it must never interrupt startup with an error.
    deps.log?.(`update check failed: ${String(e)}`);
    return;
  }
  if (!update) {
    deps.log?.("already up to date");
    return;
  }

  let proceed: boolean;
  try {
    proceed = await deps.confirm(
      `Turnstile ${update.version} is available (you have ${update.currentVersion}). ` +
        `Download and install it now? Turnstile will restart to finish.`,
      "Update available",
    );
  } catch (e) {
    deps.log?.(`update prompt failed: ${String(e)}`);
    return;
  }
  if (!proceed) return;

  // The user opted in — from here, failures are worth surfacing.
  deps.notify("info", `Downloading Turnstile ${update.version}…`);
  try {
    await update.downloadAndInstall();
  } catch (e) {
    deps.notify(
      "error",
      `Update to ${update.version} failed — ${String(e)}. ` +
        `You can download the latest version manually.`,
    );
    return;
  }

  try {
    await deps.relaunch();
  } catch (e) {
    // Installed successfully but couldn't relaunch automatically; tell the user
    // to restart rather than leaving them on the stale version silently.
    deps.log?.(`relaunch failed: ${String(e)}`);
    deps.notify(
      "info",
      `Turnstile ${update.version} is installed. Restart to use the new version.`,
    );
  }
}
