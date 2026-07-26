import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useSettingsStore } from '../../stores/settingsStore';
import { Button } from '../common/Button';
import { Select } from '../common/Select';
import { Toggle } from '../common/Toggle';
import { SettingsSection, SettingRow, SettingNote } from './SettingsSection';
import {
  ACCENTS,
  ACCENT_LABELS,
  THEMES,
  normalizeAppearance,
  type Accent,
  type Theme,
} from '../../theme';

const LANGUAGE_OPTIONS = [
  { value: 'en', label: 'English' },
  { value: 'es', label: 'Español' },
];

const THEME_LABELS: Record<Theme, string> = {
  dark: 'Dark',
  light: 'Light',
};

/** Preview swatch for one accent hue, drawn from that hue's own tokens. */
function AccentSwatch({ accent, selected }: { accent: Accent; selected: boolean }) {
  return (
    <span
      className="inline-block h-4 w-4 rounded-full"
      style={{
        background: `var(--h-${accent}-solid)`,
        boxShadow: selected ? '0 0 0 2px var(--panel), 0 0 0 3.5px var(--text)' : 'none',
      }}
      aria-hidden
    />
  );
}

export interface GeneralSettingsProps {
  onRelaunchWizard?: () => void;
}

export function GeneralSettings({ onRelaunchWizard }: GeneralSettingsProps) {
  const { settings, updateSection } = useSettingsStore();
  const general = settings.general;
  const { theme, accent } = normalizeAppearance(general.theme, general.accent);

  const [settingsPath, setSettingsPath] = useState<string | null>(null);

  useEffect(() => {
    invoke<string>('get_settings_path')
      .then(setSettingsPath)
      .catch((error) => {
        console.error('Failed to resolve settings path:', error);
        setSettingsPath(null);
      });
  }, []);

  const patchGeneral = (patch: Partial<typeof general>) => updateSection('general', patch);

  const handleReplayWizard = () => {
    // Just open it. This used to clear `has_completed_onboarding` first, which
    // meant cancelling a deliberate re-run left onboarding marked as pending --
    // so the wizard came back uninvited on the next launch. The flag is set on
    // completion and nowhere else.
    onRelaunchWizard?.();
  };

  return (
    <div className="space-y-4">
      <SettingsSection
        eyebrow="APPEARANCE"
        title="Theme and accent"
        description="Both themes are built from the same tokens, so the accent works with either one."
      >
        <SettingRow
          label="Theme"
          help={
            theme === 'light'
              ? 'Light borrows the Super Famicom shell: warm grey chrome, near-white panels.'
              : 'Dark sits on a warm charcoal rather than a blue-grey, matching the play deck.'
          }
        >
          <div className="seg" role="group" aria-label="Theme">
            {THEMES.map((value) => (
              <button
                key={value}
                type="button"
                onClick={() => patchGeneral({ theme: value })}
                className={`seg-item ${theme === value ? 'seg-item--on' : ''}`}
                aria-pressed={theme === value}
              >
                {THEME_LABELS[value]}
              </button>
            ))}
          </div>
        </SettingRow>

        <SettingRow
          label="Accent colour"
          help="Taken from the four Super Famicom face buttons. Drives buttons, focus rings and selection across the app."
        >
          <div className="seg" role="group" aria-label="Accent colour">
            {ACCENTS.map((value) => (
              <button
                key={value}
                type="button"
                onClick={() => patchGeneral({ accent: value })}
                className={`seg-item ${accent === value ? 'seg-item--on' : ''}`}
                aria-pressed={accent === value}
                title={ACCENT_LABELS[value]}
                aria-label={ACCENT_LABELS[value]}
              >
                <AccentSwatch accent={value} selected={accent === value} />
              </button>
            ))}
          </div>
        </SettingRow>
      </SettingsSection>

      <SettingsSection eyebrow="APPLICATION" title="Startup and behaviour">
        <SettingRow label="Language">
          <Select
            options={LANGUAGE_OPTIONS}
            value={general.language}
            onChange={(e) => patchGeneral({ language: e.target.value })}
            inputSize="sm"
            className="w-44"
            aria-label="Language"
          />
        </SettingRow>

        <SettingRow
          label="Show window on start"
          help="Launch to a visible window instead of starting minimised."
        >
          <Toggle
            checked={general.show_window_on_start}
            onChange={(e) => patchGeneral({ show_window_on_start: e.target.checked })}
            aria-label="Show window on start"
          />
        </SettingRow>

        <SettingRow
          label="Confirm before exit"
          help="Ask for confirmation when closing while a game is running."
        >
          <Toggle
            checked={general.confirm_on_exit}
            onChange={(e) => patchGeneral({ confirm_on_exit: e.target.checked })}
            aria-label="Confirm before exit"
          />
        </SettingRow>

        <SettingRow
          label="Setup wizard"
          help="Walk through folder, controller, video and audio setup again. Your current settings stay as they are until you finish it."
        >
          <Button variant="secondary" size="sm" onClick={handleReplayWizard}>
            Run setup wizard
          </Button>
        </SettingRow>
      </SettingsSection>

      <SettingsSection
        eyebrow="ABOUT"
        title="OxideSFC"
        description="A Super Nintendo emulator with a from-scratch core written in Rust."
      >
        <SettingRow label="Emulated machine">
          <span className="register text-ink">SNES / SUPER FAMICOM</span>
        </SettingRow>
        <SettingRow label="Native resolution">
          <span className="register text-ink">256×224</span>
        </SettingRow>
        <SettingRow label="Video timing" help="Selected automatically from each cartridge's region.">
          <span className="register text-ink">NTSC 60 Hz · PAL 50 Hz</span>
        </SettingRow>
        <SettingRow label="Audio output">
          <span className="register text-ink">32 kHz STEREO</span>
        </SettingRow>

        <SettingNote title="Where your settings live">
          {settingsPath ? (
            <code className="block break-all font-mono text-[0.75rem] text-ink">
              {settingsPath}
            </code>
          ) : (
            'Resolving the settings path…'
          )}
        </SettingNote>
      </SettingsSection>
    </div>
  );
}
