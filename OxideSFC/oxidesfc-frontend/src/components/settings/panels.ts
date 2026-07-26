/**
 * The settings panel registry.
 *
 * Kept in its own module so `settingsIndex.ts` can reference panel ids without
 * importing the React components (which would make the search index pull the
 * whole settings tree into any module that touches it).
 */

export const SETTINGS_PANELS = ['video', 'audio', 'controls', 'library', 'general'] as const;

export type SettingsPanelId = (typeof SETTINGS_PANELS)[number];

export interface SettingsPanelMeta {
  id: SettingsPanelId;
  label: string;
  /** The subsystem this panel drives, shown as the panel's own eyebrow. */
  scope: string;
}

export const SETTINGS_PANEL_META: Record<SettingsPanelId, SettingsPanelMeta> = {
  video: { id: 'video', label: 'Video', scope: 'PPU / OUTPUT' },
  audio: { id: 'audio', label: 'Audio', scope: 'S-DSP / APU' },
  controls: { id: 'controls', label: 'Controls', scope: 'JOYPAD 1-2' },
  library: { id: 'library', label: 'Library', scope: 'CARTRIDGE STORE' },
  general: { id: 'general', label: 'General', scope: 'APPLICATION' },
};
