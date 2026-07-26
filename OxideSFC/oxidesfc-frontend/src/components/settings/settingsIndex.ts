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
    section: 'Video filters',
    keywords: ['crt', 'scanlines', 'xbrz', 'hq2x', 'scale2x', 'effect', 'post'],
  },
  {
    label: 'Vertical sync',
    panel: 'video',
    section: 'Performance',
    keywords: ['vsync', 'tearing', 'refresh', 'stutter'],
  },
  {
    label: 'Frame limit',
    panel: 'video',
    section: 'Performance',
    keywords: ['fps', 'framerate', 'cap', 'unlimited', '60', '120'],
  },

  // -- Audio ----------------------------------------------------------------
  {
    label: 'Audio output',
    panel: 'audio',
    section: 'Output',
    keywords: ['mute', 'sound', 'enable', 'disable', 'silence'],
  },
  {
    label: 'Master volume',
    panel: 'audio',
    section: 'Levels',
    keywords: ['loud', 'quiet', 'gain', 'volume'],
  },
  {
    label: 'Buffer target',
    panel: 'audio',
    section: 'Buffering',
    keywords: ['latency', 'delay', 'crackle', 'pop', 'underrun', 'glitch', 'lag', 'ms'],
  },
  {
    label: 'Audio buffering',
    panel: 'audio',
    section: 'Buffering',
    keywords: ['buffer', 'smooth', 'dropouts'],
  },

  // -- Controls -------------------------------------------------------------
  {
    label: 'Keyboard input',
    panel: 'controls',
    section: 'Keyboard',
    keywords: ['keys', 'enable', 'disable'],
  },
  {
    label: 'Button mapping',
    panel: 'controls',
    section: 'Keyboard',
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
    ],
  },
  {
    label: 'Gamepad input',
    panel: 'controls',
    section: 'Gamepad',
    keywords: ['controller', 'joystick', 'pad', 'xbox', 'playstation', 'usb'],
  },
  {
    label: 'Deadzone',
    panel: 'controls',
    section: 'Gamepad',
    keywords: ['stick', 'analog', 'drift', 'sensitivity'],
  },
  {
    label: 'Gamepad profile',
    panel: 'controls',
    section: 'Gamepad',
    keywords: ['layout', 'xbox', 'playstation', 'switch', 'preset'],
  },

  // -- Library --------------------------------------------------------------
  {
    label: 'ROM folders',
    panel: 'library',
    section: 'Sources',
    keywords: ['path', 'directory', 'add', 'scan', 'games', 'roms', 'folder'],
  },
  {
    label: 'Scan on startup',
    panel: 'library',
    section: 'Scanning',
    keywords: ['auto', 'automatic', 'launch', 'refresh'],
  },
  {
    label: 'Metadata fetching',
    panel: 'library',
    section: 'Scanning',
    keywords: ['online', 'internet', 'scrape', 'info', 'download'],
  },
  {
    label: 'Artwork source',
    panel: 'library',
    section: 'Artwork',
    keywords: ['cover', 'box art', 'screenscraper', 'igdb', 'images', 'thumbnails'],
  },
  {
    label: 'Cover resolution',
    panel: 'library',
    section: 'Artwork',
    keywords: ['size', 'quality', 'thumbnail', 'storage'],
  },
  {
    label: 'Verify library',
    panel: 'library',
    section: 'Maintenance',
    keywords: ['missing', 'broken', 'clean', 'prune', 'check'],
  },
  {
    label: 'Clear library',
    panel: 'library',
    section: 'Maintenance',
    keywords: ['reset', 'delete', 'wipe', 'remove all', 'empty'],
  },

  // -- General --------------------------------------------------------------
  {
    label: 'Theme',
    panel: 'general',
    section: 'Appearance',
    keywords: ['dark', 'light', 'colour', 'color', 'bright'],
  },
  {
    label: 'Accent colour',
    panel: 'general',
    section: 'Appearance',
    keywords: ['accent', 'highlight', 'colour', 'color', 'red', 'blue', 'green', 'yellow'],
  },
  {
    label: 'Language',
    panel: 'general',
    section: 'Application',
    keywords: ['locale', 'translation', 'english', 'spanish'],
  },
  {
    label: 'Confirm before exit',
    panel: 'general',
    section: 'Application',
    keywords: ['quit', 'close', 'prompt', 'warning'],
  },
  {
    label: 'Setup wizard',
    panel: 'general',
    section: 'Application',
    keywords: ['onboarding', 'first run', 'replay', 'walkthrough', 'tutorial'],
  },
  {
    label: 'Settings file',
    panel: 'general',
    section: 'About',
    keywords: ['json', 'path', 'config', 'location', 'where', 'stored', 'backup'],
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
