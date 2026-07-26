/**
 * Appearance plumbing.
 *
 * The whole design system resolves through two attributes on <html>:
 *
 *   data-theme="dark" | "light"
 *   data-accent="red" | "yellow" | "green" | "blue"
 *
 * src/styles/tokens.css keys every colour token off those attributes, so the
 * only thing the TypeScript side has to do is keep them in step with persisted
 * settings. Nothing in the component tree should read `settings.general.theme`
 * in order to pick a colour -- that is what the tokens are for.
 */

export const THEMES = ['dark', 'light'] as const;
export type Theme = (typeof THEMES)[number];

/**
 * The accent hues are named after the Super Famicom face button each one
 * borrows: A is red, B yellow, X blue, Y green.
 */
export const ACCENTS = ['blue', 'red', 'yellow', 'green'] as const;
export type Accent = (typeof ACCENTS)[number];

export const DEFAULT_THEME: Theme = 'dark';
export const DEFAULT_ACCENT: Accent = 'blue';

/** Human labels, paired with the button each hue is taken from. */
export const ACCENT_LABELS: Record<Accent, string> = {
  blue: 'Blue (X)',
  red: 'Red (A)',
  yellow: 'Yellow (B)',
  green: 'Green (Y)',
};

export function isTheme(value: unknown): value is Theme {
  return typeof value === 'string' && (THEMES as readonly string[]).includes(value);
}

export function isAccent(value: unknown): value is Accent {
  return typeof value === 'string' && (ACCENTS as readonly string[]).includes(value);
}

/**
 * Coerce whatever is in settings.json into a valid theme/accent pair. Settings
 * files predate the accent field entirely, and `theme` has historically been a
 * free-form string on both sides of the IPC boundary, so an unrecognised value
 * has to fall back rather than produce an <html> attribute no CSS rule matches
 * (which would leave the app unstyled).
 */
export function normalizeAppearance(theme: unknown, accent: unknown): {
  theme: Theme;
  accent: Accent;
} {
  return {
    theme: isTheme(theme) ? theme : DEFAULT_THEME,
    accent: isAccent(accent) ? accent : DEFAULT_ACCENT,
  };
}

/** Write the pair onto <html>. Safe to call on every settings change. */
export function applyAppearance(theme: unknown, accent: unknown): void {
  const normalized = normalizeAppearance(theme, accent);
  const root = document.documentElement;
  root.dataset.theme = normalized.theme;
  root.dataset.accent = normalized.accent;
}
