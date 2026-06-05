/**
 * End-to-end praxis: drive the real Turnstile UI in a browser against the
 * fake backend (src/lib/fake/), which simulates the Rust side's event
 * contracts. The scenario mirrors real use: write the sqrt(2)
 * irrationality theorem, watch elaboration progress come and *go* (the
 * 1.0 highlight bug), read the goal state, and talk to the assistant.
 */
import { expect, test, type Page } from "@playwright/test";

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

test("assistant chat: echo round-trip and input disabled while busy", async ({
  page,
}) => {
  await openApp(page);
  const input = page.getByPlaceholder("Message Proof Assistant…");
  await input.fill("hello turnstile");
  await input.press("Enter");
  await expect(page.locator(".bubble.assistant").last()).toContainText(
    "[echo] hello turnstile",
    { timeout: 10_000 },
  );
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

test("opening a session loads the proof and prose", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "open_session");
  });
  await expect(page.locator(".cm-content")).toContainText("sqrt2_irrational", {
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
