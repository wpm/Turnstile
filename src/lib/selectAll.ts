// Scoped Select All (#113). Command-A / Ctrl-A must select the text of the panel
// the user is working in — not every selectable region in the window. The
// browser's native Select All ignores panel boundaries and highlights the whole
// document, which is never what's wanted in a multi-panel app.
//
// The CodeMirror panels (Formal Proof, Proof Assistant entry) already bind
// Mod-a to their own in-editor selectAll and mark the event handled, so this
// handler stays out of their way (see the defaultPrevented guard below). It
// covers the read-only HTML panels — the Goal State, the Prose Proof, and the
// Proof Assistant transcript — each of which tags its scrollable content with
// the `data-select-scope` attribute.

/** Marks an element as a Select-All scope: `<div data-select-scope>…</div>`. */
export const SELECT_SCOPE_ATTR = "data-select-scope";

const SELECT_SCOPE_SELECTOR = `[${SELECT_SCOPE_ATTR}]`;

/** True for an unmodified Command-A (macOS) or Ctrl-A (elsewhere). */
export function isSelectAllEvent(e: KeyboardEvent): boolean {
  return (
    (e.metaKey || e.ctrlKey) &&
    !e.shiftKey &&
    !e.altKey &&
    (e.key === "a" || e.key === "A")
  );
}

/** The nearest Select-All scope ancestor of a node, or null. */
function closestScope(node: Node | null): HTMLElement | null {
  let el: Node | null = node;
  // A text node (the common selection anchor) can't `.closest`; climb to its
  // owning element first.
  while (el && el.nodeType !== Node.ELEMENT_NODE) el = el.parentNode;
  return (el as Element | null)?.closest(SELECT_SCOPE_SELECTOR) ?? null;
}

/**
 * The scope that owns the current focus or selection, or null when the caret /
 * selection sits outside every registered scope (i.e. no panel is focused).
 *
 * The read-only panels aren't keyboard-focusable, so `activeElement` stays on
 * `<body>` for them; their "focus" is the collapsed selection a click leaves in
 * the text. We therefore consult the selection anchor first, then fall back to
 * `activeElement`.
 */
export function activeSelectScope(doc: Document): HTMLElement | null {
  const sel = doc.getSelection();
  const anchor = sel && sel.rangeCount > 0 ? sel.anchorNode : null;
  return closestScope(anchor) ?? closestScope(doc.activeElement);
}

/**
 * Handle a keydown for scoped Select All. Returns true when it took over the
 * Select All — the caller need do nothing more; the browser's whole-document
 * default has been suppressed.
 *
 * When a panel scope is focused, its contents are selected. When none is (the
 * caret is outside every scope), Select All is still suppressed and nothing is
 * selected — selecting the whole window is never the intent here.
 */
export function handleSelectAll(e: KeyboardEvent, doc: Document): boolean {
  if (!isSelectAllEvent(e)) return false;
  // A focused CodeMirror editor handles Mod-a itself and marks the event
  // handled; never override an in-editor Select All.
  if (e.defaultPrevented) return false;

  // Take over from the browser's whole-document Select All unconditionally.
  e.preventDefault();

  const scope = activeSelectScope(doc);
  if (!scope) return true; // no focused panel → select nothing

  const sel = doc.getSelection();
  if (!sel) return true;
  const range = doc.createRange();
  range.selectNodeContents(scope);
  sel.removeAllRanges();
  sel.addRange(range);
  return true;
}
