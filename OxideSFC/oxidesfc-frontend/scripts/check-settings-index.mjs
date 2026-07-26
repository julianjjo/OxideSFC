/**
 * Guards `settingsIndex.ts` against drifting away from the panels it indexes.
 *
 * The index powers the settings screen's search: a match navigates to the panel
 * that owns the setting. A stale entry therefore sends someone to a control that
 * has been renamed or removed, which is worse than not indexing it at all — and
 * because nothing links the two files, that drift is invisible until a user
 * searches for it.
 *
 * This checks the cheap, high-value invariant: every entry's `label` and
 * `section` must appear as a literal string in the panel it points at. It cannot
 * catch a control that exists but is missing from the index; it does catch every
 * rename and deletion, which is how the drift actually happens.
 *
 * Run with `npm run check:settings-index`.
 */

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const settingsDir = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'components', 'settings');

const PANEL_FILES = {
  video: 'VideoSettings.tsx',
  audio: 'AudioSettings.tsx',
  controls: 'ControllerSettings.tsx',
  library: 'LibrarySettings.tsx',
  general: 'GeneralSettings.tsx',
};

const indexSource = readFileSync(join(settingsDir, 'settingsIndex.ts'), 'utf8');

const panelSources = {};
for (const [panel, file] of Object.entries(PANEL_FILES)) {
  panelSources[panel] = readFileSync(join(settingsDir, file), 'utf8');
}

// Matches the entry shape the index is written in. Deliberately strict: if the
// formatting changes enough to stop matching, the count assertion at the end
// fails loudly rather than silently checking nothing.
const ENTRY = /label: '([^']+)',\s*\n\s*panel: '([^']+)',\s*\n\s*section: '([^']+)'/g;

/** Is `text` present as a single- or double-quoted literal in `source`? */
function hasLiteral(source, text) {
  return source.includes(`"${text}"`) || source.includes(`'${text}'`);
}

const problems = [];
let checked = 0;

for (const [, label, panel, section] of indexSource.matchAll(ENTRY)) {
  checked++;

  const source = panelSources[panel];
  if (!source) {
    problems.push(`unknown panel "${panel}" for entry "${label}"`);
    continue;
  }
  if (!hasLiteral(source, label)) {
    problems.push(`[${panel}] label "${label}" is not rendered by ${PANEL_FILES[panel]}`);
  }
  if (!hasLiteral(source, section)) {
    problems.push(`[${panel}] section "${section}" is not rendered by ${PANEL_FILES[panel]}`);
  }
}

const declared = (indexSource.match(/panel: '/g) ?? []).length;
if (checked !== declared) {
  problems.push(
    `parsed ${checked} entries but found ${declared} "panel:" keys — the entry format changed, so this check is no longer reading the whole index`
  );
}

if (problems.length > 0) {
  console.error(`settings index is out of sync with the panels (${problems.length} problem(s)):\n`);
  for (const problem of problems) console.error(`  - ${problem}`);
  console.error('\nUpdate settingsIndex.ts in the same change as the panel.');
  process.exit(1);
}

console.log(`settings index OK — ${checked} entries match their panels.`);
