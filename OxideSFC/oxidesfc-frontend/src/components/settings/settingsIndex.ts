/**
 * Search index for the settings screen.
 *
 * With ~30 controls spread over five panels, the hardest thing to do in
 * settings is find the one you came for -- especially when you remember what it
 * does but not which panel the author filed it under ("the blurry-pixels one"
 * lives under Video / Renderer, not under a Filters panel).
 *
 * This is a jump index rather than a live row filter: a match navigates to the
 * panel that owns the setting instead of pulling controls out of the context
 * that explains them. That keeps the panels as the single implementation of
 * each control, and it means adding a setting only requires adding a row here,
 * not restructuring anything.
 *
 * `keywords` carries the words people actually search with, including the ones
 * that do NOT appear in the visible label -- that is the whole point of the
 * field, so prefer adding synonyms over duplicating the label.
 */

import type { SettingsPanelId } from './panels';

export interface SettingsIndexEntry {
  /** Visible label of the control, as it reads in its panel. */
  label: string;
  panel: SettingsPanelId;
  /** Section heading the control sits under, shown as the result's context. */
  section: string;
  keywords: string[];
}

/**
 * MAINTENANCE: `label` and `section` must match what the panel actually renders.
 *
 * They are what a search result shows, so a stale entry sends someone to a
 * control that has been renamed or removed — worse than not indexing it at all.
 * There is no automated check (the project has no TS test runner), so this list
 * has to be updated in the same change as the panel.
 */
export const SETTINGS_INDEX: SettingsIndexEntry[] = [
  // -- Video ----------------------------------------------------------------
  {
    label: 'Graphics API',
    panel: 'video',
    section: 'Renderer',
    keywords: ['webgl', 'webgpu', 'gpu', 'backend', 'driver'],
  },
  {
    label: 'Scale mode',
    panel: 'video',
    section: 'Renderer',
    keywords: [
      'nearest',
      'bilinear',
      'xbrz',
      'hq2x',
      'blurry',
      'sharp',
      'smoothing',
      'filter',
      'pixelated',
      'interpolation',
      'upscale',
    ],
  },
  {
    label: 'Shader',
    panel: 'video',
    section: 'Video filter',
    keywords: ['crt', 'scanlines', 'curvature', 'vignette', 'effect', 'post'],
  },
  {
    label: 'Vertical sync',
    panel: 'video',
    section: 'Frame delivery',
    keywords: ['vsync', 'tearing', 'refresh', 'stutter'],
  },
  {
    label: 'Frame limit',
    panel: 'video',
    section: 'Frame delivery',
    keywords: ['fps', 'framerate', 'cap', 'unlimited', '60', '120'],
  },

  // -- Audio ----------------------------------------------------------------
  {
    label: 'Enable audio',
    panel: 'audio',
    section: 'Sound',
    keywords: ['mute', 'sound', 'audio', 'enable', 'disable', 'silence'],
  },
  {
    label: 'Volume',
    panel: 'audio',
    section: 'Sound',
    keywords: ['loud', 'quiet', 'gain', 'volume', 'music', 'sfx'],
  },
  {
    label: 'Buffer target',
    panel: 'audio',
    section: 'Buffering',
    keywords: [
      'latency',
      'delay',
      'crackle',
      'pop',
      'underrun',
      'glitch',
      'lag',
      'ms',
      'stutter',
    ],
  },
  {
    label: 'Live pipeline',
    panel: 'audio',
    section: 'Live pipeline',
    keywords: ['diagnostics', 'underruns', 'dropped', 'drc', 'stats', 'debug', 'buffer fill'],
  },

  // -- Controls -------------------------------------------------------------
  {
    label: 'Enable keyboard input',
    panel: 'controls',
    section: 'Keyboard mapping',
    keywords: ['keys', 'keyboard', 'enable', 'disable'],
  },
  {
    label: 'Keyboard mapping',
    panel: 'controls',
    section: 'Keyboard mapping',
    keywords: [
      'remap',
      'rebind',
      'bind',
      'keys',
      'controls',
      'a',
      'b',
      'x',
      'y',
      'start',
      'select',
      'dpad',
      'defaults',
    ],
  },
  {
    label: 'Enable gamepad input',
    panel: 'controls',
    section: 'Gamepad',
    keywords: ['controller', 'joystick', 'pad', 'xbox', 'playstation', 'usb'],
  },
  {
    label: 'Button layout',
    panel: 'controls',
    section: 'Gamepad',
    keywords: ['profile', 'layout', 'xbox', 'playstation', 'switch', 'preset'],
  },
  {
    label: 'Stick deadzone',
    panel: 'controls',
    section: 'Gamepad',
    keywords: ['deadzone', 'stick', 'analog', 'drift', 'sensitivity'],
  },
  {
    label: 'Input test',
    panel: 'controls',
    section: 'Gamepad',
    keywords: ['test', 'check', 'detect', 'buttons', 'axes', 'troubleshoot'],
  },

  // -- Library --------------------------------------------------------------
  {
    label: 'ROM folders',
    panel: 'library',
    section: 'ROM folders',
    keywords: ['path', 'directory', 'add', 'scan', 'games', 'roms', 'folder'],
  },
  {
    label: 'Include subfolders',
    panel: 'library',
    section: 'How folders are read',
    keywords: ['recursive', 'nested', 'subdirectory', 'depth', 'scan'],
  },
  {
    label: 'Fetch metadata',
    panel: 'library',
    section: 'How folders are read',
    keywords: ['online', 'internet', 'scrape', 'info', 'download', 'artwork'],
  },
  {
    label: 'Get cover art',
    panel: 'library',
    section: 'Cover art',
    keywords: [
      'cover',
      'covers',
      'box art',
      'boxart',
      'artwork',
      'images',
      'thumbnails',
      'libretro',
      'download',
      'missing',
    ],
  },
  {
    label: 'Re-check everything',
    panel: 'library',
    section: 'Cover art',
    keywords: ['recheck', 'retry', 'again', 'force', 'refresh', 'covers'],
  },
  {
    label: 'Cached images',
    panel: 'library',
    section: 'Cover art',
    keywords: ['cache', 'clear', 'delete', 'covers', 'disk space', 'storage'],
  },
  {
    label: 'Verify',
    panel: 'library',
    section: 'Library data',
    keywords: ['missing', 'broken', 'clean', 'prune', 'check', 'verify'],
  },
  {
    label: 'Clear',
    panel: 'library',
    section: 'Library data',
    keywords: ['reset', 'delete', 'wipe', 'remove all', 'empty', 'clear library'],
  },

  // -- General --------------------------------------------------------------
  {
    label: 'Theme',
    panel: 'general',
    section: 'Theme and accent',
    keywords: ['dark', 'light', 'colour', 'color', 'bright', 'appearance'],
  },
  {
    label: 'Accent colour',
    panel: 'general',
    section: 'Theme and accent',
    keywords: ['accent', 'highlight', 'colour', 'color', 'red', 'blue', 'green', 'yellow'],
  },
  {
    label: 'Language',
    panel: 'general',
    section: 'Startup and behaviour',
    keywords: ['locale', 'translation', 'english', 'spanish'],
  },
  {
    label: 'Show window on start',
    panel: 'general',
    section: 'Startup and behaviour',
    keywords: ['minimised', 'minimized', 'tray', 'launch', 'startup'],
  },
  {
    label: 'Confirm before exit',
    panel: 'general',
    section: 'Startup and behaviour',
    keywords: ['quit', 'close', 'prompt', 'warning'],
  },
  {
    label: 'Setup wizard',
    panel: 'general',
    section: 'Startup and behaviour',
    keywords: ['onboarding', 'first run', 'replay', 'walkthrough', 'tutorial'],
  },
  {
    label: 'Where your settings live',
    panel: 'general',
    section: 'OxideSFC',
    keywords: ['json', 'path', 'config', 'location', 'where', 'stored', 'backup', 'file'],
  },
];

/**
 * Rank index entries against a query.
 *
 * Label matches outrank keyword matches, and a prefix match outranks a
 * mid-string one, so typing "vol" puts "Master volume" above an entry that only
 * mentions volume in passing. Returns [] for a blank query so callers can treat
 * "no query" and "no results" differently -- an empty result set for a typed
 * query is a real answer that deserves a message.
 */
export function searchSettings(query: string): SettingsIndexEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];

  const scored: Array<{ entry: SettingsIndexEntry; score: number }> = [];

  for (const entry of SETTINGS_INDEX) {
    const label = entry.label.toLowerCase();
    let score = 0;

    if (label === q) score = 100;
    else if (label.startsWith(q)) score = 80;
    else if (label.includes(q)) score = 60;
    else if (entry.section.toLowerCase().includes(q)) score = 40;
    else if (entry.keywords.some((k) => k.startsWith(q))) score = 30;
    else if (entry.keywords.some((k) => k.includes(q))) score = 15;

    if (score > 0) scored.push({ entry, score });
  }

  return scored
    .sort((a, b) => b.score - a.score || a.entry.label.localeCompare(b.entry.label))
    .map((s) => s.entry);
}
