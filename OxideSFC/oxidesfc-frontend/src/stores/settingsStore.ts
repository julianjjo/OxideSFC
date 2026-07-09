import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface VideoSettings {
  vsync: boolean;
  frame_limit: string;
  renderer: string;
  shader: string;
  scale_mode: string;
}

export interface AudioSettings {
  enabled: boolean;
  volume: number;
  latency: number;
  sfx_volume: number;
  music_volume: number;
  buffering_enabled: boolean;
}

export interface ControlSettings {
  keyboard_enabled: boolean;
  gamepad_enabled: boolean;
  keyboard_mapping: Record<string, string>;
  gamepad_profile: string;
}

export interface LibrarySettings {
  folders: string[];
  scan_recursive: boolean;
  use_metadata: boolean;
  cover_resolution: string;
  artwork_source: string;
}

export interface GeneralSettings {
  language: string;
  theme: string;
  show_window_on_start: boolean;
  confirm_on_exit: boolean;
  has_completed_onboarding: boolean;
}

export interface AppSettings {
  general: GeneralSettings;
  video: VideoSettings;
  audio: AudioSettings;
  controls: ControlSettings;
  library: LibrarySettings;
}

interface SettingsState {
  settings: AppSettings;
  isLoading: boolean;
  
  // Actions
  loadSettings: () => Promise<void>;
  saveSettings: (settings: AppSettings) => Promise<void>;
  updateSettings: (partial: Partial<AppSettings>) => Promise<void>;
}

const defaultSettings: AppSettings = {
  general: {
    language: 'en',
    theme: 'dark',
    show_window_on_start: true,
    confirm_on_exit: true,
    has_completed_onboarding: false,
  },
  video: {
    vsync: true,
    frame_limit: '60',
    renderer: 'webgl',
    shader: 'none',
    scale_mode: 'bilinear',
  },
  audio: {
    enabled: true,
    volume: 1.0,
    latency: 50,
    sfx_volume: 100,
    music_volume: 100,
    buffering_enabled: true,
  },
  controls: {
    keyboard_enabled: true,
    gamepad_enabled: true,
    keyboard_mapping: {
      'ArrowUp': 'up',
      'ArrowDown': 'down',
      'ArrowLeft': 'left',
      'ArrowRight': 'right',
      'KeyZ': 'a',
      'KeyX': 'b',
      'Enter': 'start',
      'ShiftRight': 'select',
      'KeyA': 'l',
      'KeyS': 'r',
    },
    gamepad_profile: 'default',
  },
  library: {
    folders: [],
    scan_recursive: true,
    use_metadata: true,
    cover_resolution: 'medium',
    artwork_source: 'screenscraper',
  },
};

// Serializes all saves so two overlapping saveSettings/updateSettings calls
// can't race: each save only starts once the previous one's invoke() + set()
// has fully landed, so updateSettings always merges against the freshest
// state instead of a stale snapshot captured before an earlier save resolved.
let pendingSave: Promise<void> = Promise.resolve();

export const useSettingsStore = create<SettingsState>((set) => ({
  settings: defaultSettings,
  isLoading: false,

  loadSettings: async () => {
    set({ isLoading: true });
    try {
      const settings = await invoke<AppSettings>('get_settings');
      set({ settings, isLoading: false });
    } catch (error) {
      console.error('Failed to load settings:', error);
      set({ isLoading: false });
    }
  },

  saveSettings: async (settings: AppSettings) => {
    const run = pendingSave.then(async () => {
      await invoke('save_settings', { settings });
      set({ settings });
    });
    pendingSave = run.catch(() => {});
    try {
      await run;
    } catch (error) {
      console.error('Failed to save settings:', error);
      throw error;
    }
  },

  updateSettings: async (partial: Partial<AppSettings>) => {
    const run = pendingSave.then(async () => {
      const current = useSettingsStore.getState().settings;
      const updated = { ...current, ...partial };
      await invoke('save_settings', { settings: updated });
      set({ settings: updated });
    });
    pendingSave = run.catch(() => {});
    await run;
  },
}));
