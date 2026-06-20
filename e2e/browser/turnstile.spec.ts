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
  await expect(page.locator(".formal-panel .cm-content")).toHaveAttribute(
    "contenteditable",
    "true",
    { timeout: 10_000 },
  );
}

async function typeInEditor(page: Page, text: string) {
  await page.locator(".formal-panel .cm-content").click();
  await page.keyboard.insertText(text);
}

// The Proof Assistant entry is a CodeMirror instance (#98), not a <textarea>:
// locate its editable content and drive it through the keyboard, clearing any
// leftover text first so a dropped send doesn't bleed into the next message.
function assistantInput(page: Page) {
  return page.locator(".assistant-panel .cm-content");
}

async function sendAssistantMessage(page: Page, text: string) {
  await assistantInput(page).click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.press("Delete");
  await page.keyboard.insertText(text);
  await page.keyboard.press("Enter");
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
  await expect(page.locator(".formal-panel .cm-content")).toHaveAttribute(
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
  // …and the goal state panel fills in with the descent goal. The top-right
  // pane defaults to prose now, so flip it to the goal (All-Goals) view first.
  // Scope to `.goal-state` (the panel): the cursor-row card (#91) also renders
  // a `.goal-conclusion` in the Formal pane.
  await page.getByRole("button", { name: "Switch to Formal Proof" }).click();
  await expect(page.locator(".goal-state .goal-conclusion")).toContainText(
    "n * n ≠ 2 * (d * d)",
    { timeout: 5_000 },
  );
  await expect(page.locator(".goal-state .goal-turnstile")).toBeVisible();
  // The All-Goals panel renders the cache per row (#93), labelled by line.
  await expect(page.locator(".goal-state .goal-row-label")).toContainText(
    "line",
  );
});

test("a completed proof runs out of goals, not a bare ⊢ no goals", async ({
  page,
}) => {
  await openApp(page);
  // Flip the top-right pane to the goal view (prose is the default).
  await page.getByRole("button", { name: "Switch to Formal Proof" }).click();
  // A proof with no `sorry` elaborates to no goals (fake returns "no goals").
  await typeInEditor(page, "theorem t : True := trivial");

  // The panel shows a quiet empty-state — a closed proof simply runs out of
  // goals (ADR-0004); the green "proof complete" banner is gone.
  await expect(page.locator(".goal-empty")).toContainText("No goals.", {
    timeout: 5_000,
  });
  // …and does NOT render a dangling turnstile or the literal "no goals" as a
  // goal conclusion (the bug: ⊢ no goals).
  await expect(page.locator(".goal-turnstile")).toHaveCount(0);
  await expect(page.locator(".goal-conclusion")).toHaveCount(0);
});

test("the cursor-row goal card shows the after-state and hides off a fresh row", async ({
  page,
}) => {
  await openApp(page);
  await typeInEditor(page, THEOREM);

  // After typing, the cursor sits on the `sorry` line, whose cached goal is
  // fresh — the card appears in the Formal pane with the descent goal.
  const card = page.locator(".goal-card");
  await expect(card).toBeVisible({ timeout: 5_000 });
  await expect(card.locator(".goal-conclusion")).toContainText(
    "n * n ≠ 2 * (d * d)",
  );

  // Moving up off the `sorry` row lands on rows whose goal is not fresh, so the
  // card — a pure function of (cursor row, cache) — disappears.
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await expect(page.locator(".goal-card")).toHaveCount(0);
});

test("the goal card marks an unfocused multi-goal step with a +N notch", async ({
  page,
}) => {
  await openApp(page);
  // A `constructor` step splits into two goals; the cursor lands on that row.
  await typeInEditor(page, "theorem t : P ∧ Q := by\n  constructor");

  const card = page.locator(".goal-card");
  await expect(card).toBeVisible({ timeout: 5_000 });
  // The card shows the first goal; the remaining one becomes a +1 notch.
  await expect(card.locator(".goal-card-notch")).toHaveText("+1");

  // The All-Goals panel renders the same cache row in full — both goals.
  await page.getByRole("button", { name: "Switch to Formal Proof" }).click();
  await expect(page.locator(".goal-state .goal-conclusion")).toHaveCount(2);
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

  await sendAssistantMessage(page, "What is the current goal?");

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
  // Send sequentially until the transcript overflows. send() is a no-op while a
  // reply is still streaming, so a send fired too early is silently dropped
  // (this flaked in WebKit). Pace by waiting for the user bubble (one per
  // accepted send) and the matching finished assistant reply, and retry the
  // send if the user bubble didn't appear — never racing the next send against
  // an in-flight reply.
  const TURNS = 6;
  for (let i = 0; i < TURNS; i++) {
    await expect(async () => {
      await sendAssistantMessage(
        page,
        `overflow the transcript, message ${String(i)}`,
      );
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
  await sendAssistantMessage(page, "hello turnstile");
  const lastBubble = page.locator(".bubble.assistant").last();
  await expect(lastBubble).toContainText("Proof Assistant", {
    timeout: 10_000,
  });
  // The assistant must never echo the user's text back (#57/#62).
  await expect(lastBubble).not.toContainText("[echo]");
  await expect(lastBubble).not.toContainText("hello turnstile");
});

test("unicode abbreviations: a \\-escape dropdown renders glyphs and accepts into the formal editor", async ({
  page,
}) => {
  // #98: typing `\` + letters opens an autocomplete dropdown beneath the cursor
  // whose options show the *rendered glyph* (not the plain escape). Accepting
  // replaces the escape with the character.
  await openApp(page);
  await page.locator(".formal-panel .cm-content").click();
  // Type char-by-char so the eager (activateOnTyping) completion fires.
  await page.keyboard.type("\\alpha");

  const tooltip = page.locator(".cm-tooltip-autocomplete");
  await expect(tooltip).toBeVisible();
  // The list shows the live glyph alongside its escape.
  await expect(tooltip).toContainText("α");
  await expect(tooltip).toContainText("\\alpha");

  // Accept the (exact, top-ranked) match: the escape becomes the glyph.
  // CodeMirror guards Enter behind a 75 ms interaction delay (so a fast Enter
  // doesn't accept an option the user never saw); a human's pause clears it.
  await page.waitForTimeout(150);
  await page.keyboard.press("Enter");
  const content = page.locator(".formal-panel .cm-content");
  await expect(content).toContainText("α");
  await expect(content).not.toContainText("\\alpha");
});

test("unicode abbreviations: Enter accepts a glyph in the assistant entry instead of sending", async ({
  page,
}) => {
  // The same dropdown rides in the Proof Assistant entry (now a CodeMirror
  // instance). While it's open, Enter accepts the glyph rather than firing the
  // message — only a plain Enter (no dropdown) sends.
  await openApp(page);
  await assistantInput(page).click();
  await page.keyboard.type("\\to");
  await expect(page.locator(".cm-tooltip-autocomplete")).toContainText("→");

  // Past CodeMirror's 75 ms accept guard, Enter accepts the glyph (it does not
  // send) because the dropdown is open.
  await page.waitForTimeout(150);
  await page.keyboard.press("Enter");
  // The glyph landed in the entry; no message was sent.
  await expect(assistantInput(page)).toContainText("→");
  await expect(page.locator(".bubble.user")).toHaveCount(0);
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
  await expect(page.locator(".formal-panel .cm-content")).toHaveAttribute(
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

  // The chat input is locked (read-only) and shows the explanatory placeholder.
  // It's a CodeMirror editor (#98), so "disabled" is a non-editable content DOM
  // rather than a form control's disabled attribute.
  await expect(assistantInput(page)).toHaveAttribute(
    "contenteditable",
    "false",
  );
  await expect(page.locator(".assistant-panel .cm-placeholder")).toContainText(
    "Proof Assistant unavailable — set your API key in Settings",
  );
});

test("settings dialog opens from the menu, edits persist", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "settings");
  });
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  // Three tabs; the Models tab is selected first and shows the model selects.
  await expect(dialog.getByRole("tab")).toHaveCount(3);
  await expect(
    dialog.getByRole("tab", { name: "Models & API key" }),
  ).toHaveAttribute("aria-selected", "true");

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

test("settings tabs: arrow-key navigation and unsaved edits survive switching", async ({
  page,
}) => {
  await openApp(page);
  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "settings");
  });
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();

  // The Models tab holds the masked API-key field (#66).
  const keyField = dialog.locator("input[type='password']");
  await expect(keyField).toBeVisible();

  // Roving tabindex + arrow-key navigation: focusing the selected tab and
  // pressing Right moves both selection and focus to the next tab.
  const modelsTab = dialog.getByRole("tab", { name: "Models & API key" });
  const fontsTab = dialog.getByRole("tab", { name: "Font sizes" });
  const promptsTab = dialog.getByRole("tab", { name: "Prompts" });
  await modelsTab.focus();
  await page.keyboard.press("ArrowRight");
  await expect(fontsTab).toHaveAttribute("aria-selected", "true");
  await expect(fontsTab).toBeFocused();
  await page.keyboard.press("End");
  await expect(promptsTab).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Home");
  await expect(modelsTab).toHaveAttribute("aria-selected", "true");

  // Unsaved edits survive tab switches because every panel binds the shared
  // draft. Edit the key in Models, switch to Font sizes (its field hidden),
  // edit a font, then switch back — both edits are intact.
  await keyField.fill("sk-ant-draft-key");
  await fontsTab.click();
  await expect(keyField).toBeHidden();
  const editorFont = dialog
    .getByRole("tabpanel", { name: "Font sizes" })
    .locator("input[type='number']")
    .first();
  await editorFont.fill("20");
  await modelsTab.click();
  await expect(keyField).toHaveValue("sk-ant-draft-key");
  await fontsTab.click();
  await expect(editorFont).toHaveValue("20");
});

test("word wrap toggles via the View menu", async ({ page }) => {
  await openApp(page);
  const longLine = "-- " + "x".repeat(400);
  await typeInEditor(page, longLine);

  const contentWidth = async () =>
    (await page.locator(".formal-panel .cm-content").boundingBox())?.width ?? 0;
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
  // The Prose Proof pane is shown by default (upper-right).

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
  await expect(page.locator(".formal-panel .cm-content")).toContainText(
    "sqrt2_irrational",
    {
      timeout: 5_000,
    },
  );
});

test("loading a session over existing edits replaces the backend source, not concatenates it", async ({
  page,
}) => {
  // Regression: the session-load listener replaced the editor doc with a
  // transaction that the update listener then echoed back to the backend as an
  // *incremental* change — applied on top of the source the backend had already
  // set during the load. The diff (computed against the pre-load editor) double-
  // applied, leaving the loaded source concatenated with leftover old text. The
  // Proof Assistant's read_lean_source then saw two theorems glued together.
  await openApp(page);

  // The user has some prior content in the editor (and thus in the backend).
  await typeInEditor(page, "theorem old : True := sorry");
  await expect(page.locator(".formal-panel .cm-content")).toContainText(
    "theorem old",
  );

  // Loading a session sets the backend source directly and re-populates the
  // editor. The backend source must equal exactly the loaded proof.
  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "open_session");
  });
  await expect(page.locator(".formal-panel .cm-content")).toContainText(
    "sqrt2_irrational",
    {
      timeout: 5_000,
    },
  );

  const source = await page.evaluate(
    () => window.__turnstileFake?.getSource() ?? "",
  );
  // No residue of the pre-load content, and no duplicated declarations.
  expect(source).not.toContain("theorem old");
  expect(source).toContain("sqrt2_irrational");
  // The editor and the backend agree exactly.
  const editorText = await page.evaluate(
    () =>
      document.querySelector(".formal-panel .cm-content")?.textContent ?? "",
  );
  expect(source.replace(/\s+/g, " ").trim()).toBe(
    editorText.replace(/\s+/g, " ").trim(),
  );
});

test("the Proof Assistant writing source replaces the editor contents", async ({
  page,
}) => {
  // The update_lean_source tool sets the backend source and emits
  // `formal-source-updated`; the editor follows it as a programmatic update
  // (no echo back to the backend), the same mechanism a session load uses.
  await openApp(page);

  // Some prior content the assistant's write should fully replace.
  await typeInEditor(page, "theorem old : True := sorry");
  await expect(page.locator(".formal-panel .cm-content")).toContainText(
    "theorem old",
  );

  const NEW_SOURCE = "theorem fixed : True := by trivial";
  await page.evaluate((src) => {
    window.__turnstileFake?.emit("formal-source-updated", src);
  }, NEW_SOURCE);

  await expect(page.locator(".formal-panel .cm-content")).toContainText(
    "theorem fixed",
    { timeout: 5_000 },
  );
  // A complete replacement: none of the old declaration survives.
  await expect(page.locator(".formal-panel .cm-content")).not.toContainText(
    "theorem old",
  );
});

test("Command-A selects only the focused panel, not the whole app (#113)", async ({
  page,
}) => {
  await openApp(page);
  await typeInEditor(page, THEOREM);
  // Build a goal state, then show the All-Goals panel (prose is the default).
  await expect(page.locator(".cm-elaborating")).toHaveCount(0, {
    timeout: 5_000,
  });
  await page.getByRole("button", { name: "Switch to Formal Proof" }).click();
  await expect(page.locator(".goal-state .goal-conclusion")).toContainText(
    "n * n ≠ 2 * (d * d)",
    { timeout: 5_000 },
  );

  // Click into the Goal State panel and select all. Helper returns whether the
  // resulting selection is wholly contained within the goal panel.
  const selectionScopedToGoal = async () => {
    await page.locator(".goal-state").click();
    await page.keyboard.press("ControlOrMeta+a");
    return page.evaluate(() => {
      const sel = window.getSelection();
      if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return false;
      const goal = document.querySelector(".goal-state");
      return goal?.contains(sel.getRangeAt(0).commonAncestorContainer) ?? false;
    });
  };
  expect(await selectionScopedToGoal()).toBe(true);

  // The selection is the goal text — and it does NOT bleed into the formal
  // editor's source (the whole-app Select All bug).
  const selected = await page.evaluate(
    () => window.getSelection()?.toString() ?? "",
  );
  expect(selected).toContain("n * n ≠ 2 * (d * d)");
  expect(selected).not.toContain("sqrt2_irrational");
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
  // Start on the prose view (the default); switch to the goal view and back.
  await expect(page.locator(".prose-proof")).toBeVisible();

  await page.getByRole("button", { name: "Switch to Formal Proof" }).click();
  await expect(page.locator(".prose-proof")).toHaveCount(0);
  await expect(page.locator(".goal-state")).toBeVisible();

  await page.getByRole("button", { name: "Switch to Prose Proof" }).click();
  await expect(page.locator(".prose-proof")).toBeVisible();
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
  await expect(page.locator(".formal-panel .cm-content")).toContainText(
    "scratch",
  );

  await page.evaluate(() => {
    window.__turnstileFake?.emit("menu-event", "new_session");
  });
  await expect(page.locator(".formal-panel .cm-content")).not.toContainText(
    "scratch",
    {
      timeout: 5_000,
    },
  );
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
  await expect(page.locator(".formal-panel .cm-content")).toContainText(
    "sqrt2_irrational",
    {
      timeout: 5_000,
    },
  );
});
