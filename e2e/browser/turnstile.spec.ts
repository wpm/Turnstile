/**
 * End-to-end browser suite: drive the real Turnstile UI in a browser against
 * the fake backend (src/lib/fake/), which simulates the Rust side's event
 * contracts. The scenario mirrors real use: write the sqrt(2)
 * irrationality theorem, watch elaboration progress come and *go* (the
 * 1.0 highlight bug), read the goal state, and talk to the assistant.
 */
import { expect, test, type Page } from "@playwright/test";

// The fake backend (src/lib/fake/backend.ts) exposes this test hook on
// `window`. Its own `declare global` augmentation is out of scope here (this
// spec doesn't import the module), so mirror the shape for type-checking.
declare global {
  interface Window {
    __turnstileFake?: {
      emit: (event: string, payload: unknown) => void;
      getSource: () => string;
      setAutosave: (v: boolean) => void;
    };
  }
}

const THEOREM = [
  "theorem sqrt2_irrational (n d : Nat) (hd : d ≠ 0) :",
  "    n * n ≠ 2 * (d * d) := by",
  "  sorry",
].join("\n");

async function openApp(page: Page) {
  await page.goto("/");
  // Editor becomes editable once the fake LSP reports connected.
  await expect(page.locator(".cm-content")).toHaveAttribute(
    "contenteditable",
    "true",
    { timeout: 10_000 },
  );
}

async function typeInEditor(page: Page, text: string) {
  await page.locator(".cm-content").click();
  await page.keyboard.insertText(text);
}

test("status bar reports a connected Lean server", async ({ page }) => {
  await openApp(page);
  await expect(page.getByRole("status")).toContainText("Lean: connected");
});

test("recovers connected status emitted before the listener subscribed", async ({
  page,
}) => {
  // Regression: when the backend reaches "connected" before the frontend's
  // turnstile-message listener is registered (e.g. Mathlib already built),
  // the live event is dropped. Without get_lsp_status recovery the UI stays
  // stuck on "Starting…" with a read-only editor. `?eager-lsp=1` makes the
  // fake announce connected at load, before any listener exists.
  await page.goto("/?eager-lsp=1");
  await expect(page.locator(".cm-content")).toHaveAttribute(
    "contenteditable",
    "true",
    { timeout: 10_000 },
  );
  await expect(page.getByRole("status")).toContainText("Lean: connected");
});

test("typing a theorem shows processing highlights that clear on elaborationDone", async ({
  page,
}) => {
  await openApp(page);
  await typeInEditor(page, THEOREM);

  // Progress highlight appears while the fake Lean elaborates…
  await expect(page.locator(".cm-elaborating").first()).toBeVisible();
  // …and clears when elaboration completes (regression: highlights used
  // to pulse forever on stale lines).
  await expect(page.locator(".cm-elaborating")).toHaveCount(0, {
    timeout: 5_000,
  });
});

test("sorry produces a warning annotation and the goal state", async ({
  page,
}) => {
  await openApp(page);
  await typeInEditor(page, THEOREM);

  // Diagnostic squiggle from the sorry…
  await expect(page.locator(".cm-diag-warning").first()).toBeVisible({
    timeout: 5_000,
  });
  // …keyword tokens from semantic highlighting…
  await expect(page.locator(".cm-tok-keyword").first()).toBeVisible();
  // …and the goal state panel fills in with the descent goal.
  await expect(page.locator(".goal-conclusion")).toContainText(
    "n * n ≠ 2 * (d * d)",
    { timeout: 5_000 },
  );
  await expect(page.locator(".goal-turnstile")).toBeVisible();
});

test("a completed proof shows 'proof complete', not a bare ⊢ no goals", async ({
  page,
}) => {
  await openApp(page);
  // A proof with no `sorry` elaborates to no goals (fake returns "no goals").
  await typeInEditor(page, "theorem t : True := trivial");

  // The panel reports completion…
  await expect(page.locator(".goal-complete")).toContainText("proof complete", {
    timeout: 5_000,
  });
  // …and does NOT render a dangling turnstile or the literal "no goals" as a
  // goal conclusion (the bug: ⊢ no goals).
  await expect(page.locator(".goal-turnstile")).toHaveCount(0);
  await expect(page.locator(".goal-conclusion")).toHaveCount(0);
});

test("assistant chat: streams a reply about the goal and renders math", async ({
  page,
}) => {
  await openApp(page);
  await typeInEditor(page, THEOREM);
  // Wait for elaboration so the fake backend has a goal state.
  await expect(page.locator(".cm-elaborating")).toHaveCount(0, {
    timeout: 5_000,
  });

  const input = page.getByPlaceholder("Message Proof Assistant…");
  await input.fill("What is the current goal?");
  await input.press("Enter");

  // User bubble appears immediately.
  await expect(page.locator(".bubble.user")).toContainText(
    "What is the current goal?",
  );
  // Assistant reply streams in and includes rendered KaTeX math.
  await expect(page.locator(".bubble.assistant .katex").first()).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.locator(".bubble.assistant")).toContainText(
    "infinite descent",
  );
});

test("assistant chat: bubbles hug their text whether or not the transcript overflows", async ({
  page,
}) => {
  // Regression: in WebKit (the macOS Tauri webview) the transcript's flex rows
  // got a stretched used height — too short, clipping the bubble's text when
  // the turns overflowed; too tall, leaving empty space below the text when
  // they didn't. The fix pins each row to its content height
  // (`flex: 0 0 auto; height: fit-content`) so the bubble is exactly as tall as
  // its text and the transcript scrolls. Chromium doesn't reproduce the WebKit
  // stretch, so assert the CSS contract plus that no bubble clips OR balloons
  // past its own text.
  await openApp(page);
  const input = page.getByPlaceholder("Message Proof Assistant…");
  // Send sequentially until the transcript overflows. send() is a no-op while a
  // reply is still streaming, so a send fired too early is silently dropped
  // (this flaked in WebKit). Pace by waiting for the user bubble (one per
  // accepted send) and the matching finished assistant reply, and retry the
  // send if the user bubble didn't appear — never racing the next send against
  // an in-flight reply.
  const TURNS = 6;
  for (let i = 0; i < TURNS; i++) {
    await expect(async () => {
      await input.fill(`overflow the transcript, message ${String(i)}`);
      await input.press("Enter");
      await expect(page.locator(".bubble.user")).toHaveCount(i + 1, {
        timeout: 2_000,
      });
    }).toPass({ timeout: 15_000 });
    // Reply finished: the streaming bubble has settled into a stored turn, so
    // the assistant count matches the user count and no "thinking" dots remain.
    await expect(page.locator(".thinking")).toHaveCount(0, { timeout: 10_000 });
    await expect(page.locator(".bubble.assistant")).toHaveCount(i + 1, {
      timeout: 10_000,
    });
  }

  // The CSS contract that defeats the WebKit stretch: rows are neither
  // flex-grown nor -shrunk, so they take their content height. (computed
  // `height` resolves to a used pixel value, not the `fit-content` keyword, so
  // the behavioral check below is what proves the sizing.)
  const row = page.locator(".bubble-row").first();
  expect(await row.evaluate((el) => getComputedStyle(el).flexGrow)).toBe("0");
  expect(await row.evaluate((el) => getComputedStyle(el).flexShrink)).toBe("0");

  // Every assistant bubble is exactly as tall as its rendered text: it neither
  // clips (box shorter than content) nor balloons (box taller than the text
  // extent + padding).
  const bad = await page.locator(".bubble.assistant").evaluateAll(
    (els) =>
      els.filter((el) => {
        const clipped = el.clientHeight < el.scrollHeight - 1;
        const kids = [...el.children];
        const cs = getComputedStyle(el);
        const pad = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom);
        const top = Math.min(...kids.map((k) => k.getBoundingClientRect().top));
        const bot = Math.max(
          ...kids.map((k) => k.getBoundingClientRect().bottom),
        );
        const textExtent = bot - top + pad;
        const ballooned = el.getBoundingClientRect().height > textExtent + 6;
        return clipped || ballooned;
      }).length,
  );
  expect(bad).toBe(0);
});

test("assistant chat: replies without echoing the user's message", async ({
  page,
}) => {
  await openApp(page);
  const input = page.getByPlaceholder("Message Proof Assistant…");
  await input.fill("hello turnstile");
  await input.press("Enter");
  const lastBubble = page.locator(".bubble.assistant").last();
  await expect(lastBubble).toContainText("Proof Assistant", {
    timeout: 10_000,
  });
  // The assistant must never echo the user's text back (#57/#62).
  await expect(lastBubble).not.toContainText("[echo]");
  await expect(lastBubble).not.toContainText("hello turnstile");
});

test("status bar reports a connected Proof Assistant", async ({ page }) => {
  await openApp(page);
  await expect(page.getByRole("status")).toContainText(
    "Proof Assistant: connected",
  );
});

test("disconnected assistant: error toast names the fix and the input is disabled", async ({
  page,
}) => {
  // Simulate a stored-but-rejected key: a key is present (so the first-run modal
  // doesn't pop), but the assistant is disconnected.
  await page.goto("/?assistant=disconnected:keyRejected");
  await expect(page.locator(".cm-content")).toHaveAttribute(
    "contenteditable",
    "true",
    { timeout: 10_000 },
  );

  // Status bar shows disconnected.
  await expect(page.getByRole("status")).toContainText(
    "Proof Assistant: disconnected",
  );

  // An error toast names the cause and the fix.
  const toast = page.locator(".toast--error");
  await expect(toast).toBeVisible({ timeout: 10_000 });
  await expect(toast).toContainText("API key was rejected");
  await expect(toast).toContainText("Settings");

  // The chat input is disabled with an explanatory placeholder.
  const input = page.getByPlaceholder(
    "Proof Assistant unavailable — set your API key in Settings",
  );
  await expect(input).toBeDisabled();
});

test("settings dialog opens from the menu, edits persist", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "settings");
  });
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  // Change the assistant model and save.
  await dialog.locator("select").first().selectOption("claude-sonnet-4-6");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(dialog).not.toBeVisible();

  // Reopen: the choice persisted in the (fake) backend.
  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "settings");
  });
  await expect(
    page.getByRole("dialog", { name: "Settings" }).locator("select").first(),
  ).toHaveValue("claude-sonnet-4-6");
});

test("word wrap toggles via the View menu", async ({ page }) => {
  await openApp(page);
  const longLine = "-- " + "x".repeat(400);
  await typeInEditor(page, longLine);

  const contentWidth = async () =>
    (await page.locator(".cm-content").boundingBox())?.width ?? 0;
  const before = await contentWidth();

  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "toggle_word_wrap");
  });
  await expect
    .poll(contentWidth, { timeout: 5_000 })
    .toBeLessThan(before - 100);
});

test("prose proof blinks while generation is in flight, then settles", async ({
  page,
}) => {
  await openApp(page);
  // Switch the bottom panel to the Prose Proof view.
  await page.getByRole("button", { name: "Switch to Prose Proof" }).click();

  // Typing a valid proof triggers a prose regeneration; the panel blinks…
  await typeInEditor(page, "theorem t : True := trivial");
  await expect(page.locator(".prose-generating")).toBeVisible({
    timeout: 5_000,
  });
  // …and stops blinking once the (fake) translation completes, with prose shown.
  await expect(page.locator(".prose-generating")).toHaveCount(0, {
    timeout: 5_000,
  });
  await expect(page.locator(".prose-proof")).toContainText("Prose for goal");
});

test("opening a session loads the proof and prose", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "open_session");
  });
  await expect(page.locator(".cm-content")).toContainText("sqrt2_irrational", {
    timeout: 5_000,
  });
});

test("theme toggle switches the document between light and dark", async ({
  page,
}) => {
  await openApp(page);
  const html = page.locator("html");
  await expect(html).not.toHaveClass(/dark/);

  await page.getByRole("button", { name: "Switch to dark mode" }).click();
  await expect(html).toHaveClass(/dark/);

  await page.getByRole("button", { name: "Switch to light mode" }).click();
  await expect(html).not.toHaveClass(/dark/);
});

test("proof view toggle round-trips between formal and prose", async ({
  page,
}) => {
  await openApp(page);
  // Start on the formal view; switch to prose and back.
  await page.getByRole("button", { name: "Switch to Prose Proof" }).click();
  await expect(page.locator(".prose-proof")).toBeVisible();

  await page.getByRole("button", { name: "Switch to Formal Proof" }).click();
  await expect(page.locator(".prose-proof")).toHaveCount(0);
});

test("an error in the source shows an error squiggle and gutter mark", async ({
  page,
}) => {
  await openApp(page);
  // The fake backend flags the token `oops` as an error diagnostic.
  await typeInEditor(page, "theorem t := oops");

  await expect(page.locator(".cm-diag-error").first()).toBeVisible({
    timeout: 5_000,
  });
  await expect(page.locator(".cm-diag-gutter--error").first()).toBeVisible({
    timeout: 5_000,
  });
});

test("a showMessage notification surfaces a dismissable toast", async ({
  page,
}) => {
  await openApp(page);
  await page.evaluate(() => {
    window.__turnstileFake?.emit("turnstile-message", {
      type: "showMessage",
      severity: "error",
      message: "something went wrong",
    });
  });

  const toast = page.getByRole("alert");
  await expect(toast).toContainText("something went wrong");

  await toast.getByRole("button", { name: "Dismiss" }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("new session clears the editor", async ({ page }) => {
  await openApp(page);
  await typeInEditor(page, "theorem scratch : True := trivial");
  await expect(page.locator(".cm-content")).toContainText("scratch");

  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "new_session");
  });
  await expect(page.locator(".cm-content")).not.toContainText("scratch", {
    timeout: 5_000,
  });
});

test("autosave recovery prompt restores the session", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(() => window.__turnstileFake !== undefined);
  await page.evaluate(() => {
    window.__turnstileFake?.setAutosave(true);
  });
  await page.reload();
  const dialog = page.getByRole("dialog", { name: "Restore session" });
  await expect(dialog).toBeVisible({ timeout: 10_000 });
  await dialog.getByRole("button", { name: "Restore" }).click();
  await expect(page.locator(".cm-content")).toContainText("sqrt2_irrational", {
    timeout: 5_000,
  });
});
