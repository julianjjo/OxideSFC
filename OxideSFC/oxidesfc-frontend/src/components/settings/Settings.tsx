import { useEffect, useMemo, useRef, useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import { Input } from '../common/Input';
import {
  IconSearch,
  IconDisplay,
  IconAudio,
  IconGamepad,
  IconDatabase,
  IconSliders,
} from '../common/icons';
import { VideoSettings } from './VideoSettings';
import { AudioSettings } from './AudioSettings';
import { ControllerSettings } from './ControllerSettings';
import { LibrarySettings } from './LibrarySettings';
import { GeneralSettings } from './GeneralSettings';
import { SETTINGS_PANELS, SETTINGS_PANEL_META, type SettingsPanelId } from './panels';
import { searchSettings } from './settingsIndex';

export interface SettingsProps {
  onRelaunchWizard?: () => void;
}

const PANEL_ICONS: Record<SettingsPanelId, React.ReactNode> = {
  video: <IconDisplay />,
  audio: <IconAudio />,
  controls: <IconGamepad />,
  library: <IconDatabase />,
  general: <IconSliders />,
};

/**
 * Settings.
 *
 * This screen used to be a single 240-line file with raw checkboxes that
 * surfaced eight of the app's settings. The other twenty-odd already existed --
 * fully built, in `VideoSettings`, `AudioSettings`, `ControllerSettings` and
 * `LibrarySettings`, exported from this directory's index and rendered by
 * nothing. Key remapping, gamepad testing, ROM folder management and library
 * maintenance were all unreachable from the UI.
 *
 * So this file is now only a shell: a panel list, a search index that jumps to
 * the panel owning a setting, and a scroll container. Every control lives in the
 * panel that owns it, which is what makes adding one a one-file change.
 */
export function Settings({ onRelaunchWizard }: SettingsProps) {
  const { loadSettings, isLoading } = useSettingsStore();
  const [panel, setPanel] = useState<SettingsPanelId>('video');
  const [query, setQuery] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  // Each panel keeps its own scroll position conceptually, but the container is
  // shared -- reset to the top on switch so a new panel never opens mid-way
  // down.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
  }, [panel]);

  const results = useMemo(() => searchSettings(query), [query]);
  const searching = query.trim().length > 0;

  const jumpTo = (target: SettingsPanelId) => {
    setPanel(target);
    setQuery('');
    searchRef.current?.blur();
  };

  const meta = SETTINGS_PANEL_META[panel];

  return (
    <div className="flex h-full min-w-0">
      {/* Panel list ------------------------------------------------------- */}
      <div className="flex w-56 flex-none flex-col border-r border-line bg-panel">
        <div className="px-4 pb-3 pt-5">
          <h1 className="display-lg text-ink">Settings</h1>
        </div>

        <div className="px-3 pb-3">
          <Input
            ref={searchRef}
            inputSize="sm"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search settings"
            aria-label="Search settings"
            leftIcon={<IconSearch size={15} />}
            onKeyDown={(e) => {
              if (e.key === 'Escape') setQuery('');
              // Enter takes the top hit -- the fastest path when you already
              // know what you typed matches one thing.
              if (e.key === 'Enter' && results.length > 0) jumpTo(results[0].panel);
            }}
          />
        </div>

        <nav className="min-h-0 flex-1 overflow-y-auto px-3 pb-3" aria-label="Settings sections">
          {searching ? (
            <div>
              <p className="microlabel px-1 pb-2 pt-1">
                {results.length} {results.length === 1 ? 'match' : 'matches'}
              </p>
              {results.length === 0 ? (
                <p className="px-1 text-[0.8125rem] leading-relaxed text-mute">
                  Nothing matches “{query.trim()}”. Try the name of the hardware
                  it affects, like <span className="text-ink">shader</span> or{' '}
                  <span className="text-ink">gamepad</span>.
                </p>
              ) : (
                <ul className="space-y-0.5">
                  {results.map((entry) => (
                    <li key={`${entry.panel}-${entry.label}`}>
                      <button
                        type="button"
                        onClick={() => jumpTo(entry.panel)}
                        className="w-full rounded-md px-2 py-1.5 text-left transition-colors hover:bg-raised"
                      >
                        <span className="block text-[0.8125rem] font-semibold text-ink">
                          {entry.label}
                        </span>
                        <span className="microlabel">
                          {SETTINGS_PANEL_META[entry.panel].label} · {entry.section}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ) : (
            <ul className="space-y-0.5">
              {SETTINGS_PANELS.map((id) => {
                const item = SETTINGS_PANEL_META[id];
                const active = panel === id;
                return (
                  <li key={id}>
                    <button
                      type="button"
                      onClick={() => setPanel(id)}
                      aria-current={active ? 'true' : undefined}
                      className={`flex w-full items-center gap-2.5 rounded-md px-2 py-2 text-left text-sm font-semibold transition-colors ${
                        active
                          ? 'bg-accent-soft text-accent-text'
                          : 'text-dim hover:bg-raised hover:text-ink'
                      }`}
                    >
                      <span className="flex-none">{PANEL_ICONS[id]}</span>
                      {item.label}
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </nav>
      </div>

      {/* Active panel ----------------------------------------------------- */}
      <div ref={scrollRef} className="min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-3xl px-6 pb-12 pt-6">
          <header className="mb-4 flex items-baseline justify-between gap-4">
            <div>
              <p className="eyebrow">{meta.scope}</p>
              <h2 className="display-lg mt-1 text-ink">{meta.label}</h2>
            </div>
            {isLoading && <span className="hint">loading…</span>}
          </header>

          {panel === 'video' && <VideoSettings />}
          {panel === 'audio' && <AudioSettings />}
          {panel === 'controls' && <ControllerSettings />}
          {panel === 'library' && <LibrarySettings />}
          {panel === 'general' && <GeneralSettings onRelaunchWizard={onRelaunchWizard} />}
        </div>
      </div>
    </div>
  );
}
