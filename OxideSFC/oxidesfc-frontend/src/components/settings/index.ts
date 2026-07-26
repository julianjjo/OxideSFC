// The shell that mounts every panel below.
export { Settings, type SettingsProps } from './Settings';

// Panels. These were all exported here before too -- and rendered by nothing,
// because the old Settings screen reimplemented a fraction of them inline.
export { VideoSettings } from './VideoSettings';
export { AudioSettings } from './AudioSettings';
export { ControllerSettings } from './ControllerSettings';
export { LibrarySettings } from './LibrarySettings';
export { GeneralSettings, type GeneralSettingsProps } from './GeneralSettings';

// Shared panel chrome, so a new panel matches the others by construction.
export {
  SettingsSection,
  SettingRow,
  SettingBlock,
  SettingNote,
} from './SettingsSection';

export { SETTINGS_PANELS, SETTINGS_PANEL_META, type SettingsPanelId } from './panels';
export { SETTINGS_INDEX, searchSettings, type SettingsIndexEntry } from './settingsIndex';
