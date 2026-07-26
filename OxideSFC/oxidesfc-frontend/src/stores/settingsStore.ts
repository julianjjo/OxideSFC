import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { applyAppearance } from '../theme';
import { repairKeyboardMapping } from '../domain/keyLabel';

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
  /** Analog-stick deadzone, 0.0-0.5 of full deflection. */
  gamepad_deadzone: number;
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
  /**
   * UI accent hue: 'red' | 'yellow' | 'green' | 'blue' (see src/theme.ts).
   * Typed as a plain string to match `theme` and the Rust struct, which both
   * predate any narrowing; validate through `isAccent`/`normalizeAppearance`
   * rather than trusting the value.
   */
  accent: string;
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
  /**
   * True once `loadSettings` has finished at least one round-trip, successful or
   * not.
   *
   * Needed because `settings` starts out as `defaultSettings`, which is
   * indistinguishable from a real load that happens to match the defaults. Any
   * decision that depends on a *persisted* value has to wait for this rather
   * than checking `isLoading`, which is false before the first load is even
   * kicked off. That distinction was the first-run wizard bug: the flag it keys
   * off (`has_completed_onboarding`) defaults to false, so every returning user
   * got the setup wizard on launch.
   */
  hasLoaded: boolean;

  // Actions
  loadSettings: () => Promise<void>;
  saveSettings: (settings: AppSettings) => Promise<void>;
  updateSettings: (partial: Partial<AppSettings>) => Promise<void>;
  /**
   * Merge a patch into one section, resolved against the freshest state.
   *
   * **Prefer this over `saveSettings` from UI code.** `saveSettings` takes a
   * whole settings object, so a caller has to build it by spreading the
   * `settings` it captured at render time. Two changes dispatched before the
   * first re-render then both spread from the same stale snapshot, and the
   * second silently reverts the first — flipping the theme and the accent in
   * quick succession would lose the theme. The store already serialises the
   * *writes*; this makes the *reads* fresh too, by merging inside that chain.
   */
  updateSection: <K extends keyof AppSettings>(
    section: K,
    patch: Partial<AppSettings[K]>
  ) => Promise<void>;
}

const defaultSettings: AppSettings = {
  general: {
    language: 'en',
    theme: 'dark',
    show_window_on_start: true,
    confirm_on_exit: true,
    has_completed_onboarding: false,
    accent: 'blue',
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
    // Buffer target the audio worklet's dynamic rate control regulates
    // towards. This has to stay in step with AudioService's own default: 50
    // is the value that measurably underran during bursty stepping, and
    // because `50` is truthy, EmulatorView's `settings.audio?.latency || 60`
    // fallback never applied -- every user ran at the known-bad value.
    latency: 60,
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
      'KeyA': 'x',
      'KeyS': 'y',
      'KeyQ': 'l',
      'KeyW': 'r',
      'Enter': 'start',
      'ShiftRight': 'select',
    },
    gamepad_profile: 'default',
    gamepad_deadzone: 0.1,
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

/**
 * Push theme/accent onto <html> whenever settings land, from any path.
 *
 * This lives in the store rather than in a component effect so appearance can
 * never lag the persisted value: every mutation funnels through here, including
 * saves made from screens that are not themselves mounted under a theme
 * provider (the welcome wizard, for instance).
 */
function syncAppearance(settings: AppSettings): void {
  applyAppearance(settings.general?.theme, settings.general?.accent);
}

export const useSettingsStore = create<SettingsState>((set) => ({
  settings: defaultSettings,
  isLoading: false,
  hasLoaded: false,

  loadSettings: async () => {
    set({ isLoading: true });
    try {
      const loaded = await invoke<AppSettings>('get_settings');

      // Repair a keyboard_mapping stored the wrong way round by an older build's
      // welcome wizard, and write the correction straight back, so the fix is
      // permanent and the running emulator picks the bindings up. Without this,
      // affected users keep playing on the built-in defaults while the panel
      // shows every button as unbound.
      const repair = repairKeyboardMapping(loaded.controls?.keyboard_mapping);
      const settings: AppSettings = repair.repaired
        ? { ...loaded, controls: { ...loaded.controls, keyboard_mapping: repair.mapping } }
        : loaded;

      set({ settings, isLoading: false, hasLoaded: true });
      syncAppearance(settings);

      if (repair.repaired) {
        console.warn('Repaired an inverted keyboard mapping in saved settings.');
        // Fire-and-forget: a failed write just means the repair is redone next
        // launch, which is harmless.
        invoke('save_settings', { settings }).catch((error) => {
          console.error('Failed to persist repaired keyboard mapping:', error);
        });
      }
    } catch (error) {
      console.error('Failed to load settings:', error);
      // `hasLoaded` is set even on failure: callers waiting on it need to know
      // the attempt is over, and a failed read leaves the defaults in place,
      // which is the best answer available.
      set({ isLoading: false, hasLoaded: true });
    }
  },

  saveSettings: async (settings: AppSettings) => {
    // Repaint before the round-trip completes: appearance changes are the one
    // kind of setting whose effect the user is looking straight at, and waiting
    // on a disk write to see a colour change reads as lag.
    syncAppearance(settings);
    const run = pendingSave.then(async () => {
      await invoke('save_settings', { settings });
      set({ settings });
    });
    pendingSave = run.catch(() => {});
    try {
      await run;
    } catch (error) {
      console.error('Failed to save settings:', error);
      // The optimistic repaint above has to be undone, or the UI keeps showing
      // an appearance that was never persisted.
      syncAppearance(useSettingsStore.getState().settings);
      throw error;
    }
  },

  updateSection: async (section, patch) => {
    // Repaint appearance changes before the disk round-trip: this is the one
    // kind of setting whose effect the user is looking straight at. Derived from
    // the live state plus the patch, not from a caller's snapshot.
    if (section === 'general') {
      const general = {
        ...useSettingsStore.getState().settings.general,
        ...(patch as Partial<GeneralSettings>),
      };
      applyAppearance(general.theme, general.accent);
    }

    const run = pendingSave.then(async () => {
      const current = useSettingsStore.getState().settings;
      const updated: AppSettings = {
        ...current,
        [section]: { ...current[section], ...patch },
      };
      await invoke('save_settings', { settings: updated });
      set({ settings: updated });
      syncAppearance(updated);
    });
    pendingSave = run.catch(() => {});
    try {
      await run;
    } catch (error) {
      console.error('Failed to save settings:', error);
      syncAppearance(useSettingsStore.getState().settings);
      throw error;
    }
  },

  updateSettings: async (partial: Partial<AppSettings>) => {
    const run = pendingSave.then(async () => {
      const current = useSettingsStore.getState().settings;
      const updated = { ...current, ...partial };
      await invoke('save_settings', { settings: updated });
      set({ settings: updated });
      syncAppearance(updated);
    });
    pendingSave = run.catch(() => {});
    await run;
  },
}));
