/**
 * Printable labels for `KeyboardEvent.code` values.
 *
 * Shared by the controller settings panel and the welcome wizard so a key reads
 * the same in both. Both previously had their own version built from chained
 * `.replace()` calls, and both were wrong in the same way: once `'ArrowLeft'`
 * had `'Arrow'` stripped it became `'Left'`, which the *next* replace in the
 * chain rewrote to `'L-'`. Every arrow key displayed as a dash form.
 *
 * An explicit table plus prefix stripping is order-independent, so that class of
 * bug cannot recur.
 */

const KEY_LABELS: Record<string, string> = {
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Enter: 'Enter',
  NumpadEnter: 'Num Enter',
  Space: 'Space',
  Escape: 'Esc',
  ShiftLeft: 'L Shift',
  ShiftRight: 'R Shift',
  ControlLeft: 'L Ctrl',
  ControlRight: 'R Ctrl',
  AltLeft: 'L Alt',
  AltRight: 'R Alt',
  Backspace: 'Backspace',
  Tab: 'Tab',
  CapsLock: 'Caps',
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
};

/** Label for a single key code. Unknown codes are shown verbatim. */
export function formatKeyCode(code: string): string {
  if (!code) return '';
  if (KEY_LABELS[code]) return KEY_LABELS[code];
  if (code.startsWith('Key')) return code.slice(3);
  if (code.startsWith('Digit')) return code.slice(5);
  if (code.startsWith('Numpad')) return `Num ${code.slice(6)}`;
  return code;
}

/**
 * Invert a key-code -> button-name map into button-name -> key-code.
 *
 * The persisted `keyboard_mapping` is keyed by key code because that is what
 * arrives on a KeyboardEvent, but every UI that renders bindings needs the
 * opposite direction ("which key is bound to B?").
 */
export function bindingsByButton(
  mapping: Record<string, string>
): Record<string, string> {
  const byButton: Record<string, string> = {};
  for (const [code, button] of Object.entries(mapping || {})) {
    byButton[button] = code;
  }
  return byButton;
}

/** The twelve SNES button names a mapping's *values* are drawn from. */
const SNES_BUTTON_NAMES = new Set([
  'up',
  'down',
  'left',
  'right',
  'a',
  'b',
  'x',
  'y',
  'l',
  'r',
  'start',
  'select',
]);

/**
 * Repair a `keyboard_mapping` that was persisted the wrong way round.
 *
 * The welcome wizard used to save its own `button -> keyCode` map straight into
 * `settings.controls.keyboard_mapping`, which is defined as `keyCode -> button`
 * (a cast hid the mismatch -- see WelcomeWizard.handleComplete). Anyone who
 * finished onboarding on an older build has an inverted map on disk right now.
 *
 * It is invisible in play, which is what makes the repair worth doing: EmulatorView
 * looks each entry's key up in its key-code table, finds nothing, and silently
 * falls back to the built-in defaults. So the user's chosen bindings are simply
 * never applied, and nothing anywhere reports a problem.
 *
 * Direction is decided by which side of the pairs carries button names. A correct
 * map has them as values; an inverted one has them as keys. Ambiguous or empty
 * input is left untouched -- a wrong guess here would scramble good data.
 */
export function repairKeyboardMapping(mapping: Record<string, string> | undefined): {
  mapping: Record<string, string>;
  repaired: boolean;
} {
  const entries = Object.entries(mapping || {});
  if (entries.length === 0) return { mapping: mapping || {}, repaired: false };

  let keysAreButtons = 0;
  let valuesAreButtons = 0;
  for (const [key, value] of entries) {
    if (SNES_BUTTON_NAMES.has(key)) keysAreButtons++;
    if (SNES_BUTTON_NAMES.has(value)) valuesAreButtons++;
  }

  if (keysAreButtons <= valuesAreButtons) {
    return { mapping: mapping || {}, repaired: false };
  }

  const flipped: Record<string, string> = {};
  for (const [button, code] of entries) {
    // Skip blank codes rather than creating a '' key, which would then swallow
    // whichever button came last.
    if (code) flipped[code] = button;
  }
  return { mapping: flipped, repaired: true };
}
