import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface GameInfo {
  id: string;
  title: string;
  file_path: string;
  file_size: number;
  rom_type: string;
  rom_size: number;
  sram_size: number;
  region: string;
  is_valid: boolean;
  validation_errors: string[];
  /** Non-fatal findings (e.g. unfinalized checksum on beta dumps/ROM hacks):
   *  the ROM loaded and runs, but this is worth surfacing to the user. */
  validation_warnings: string[];
}

export interface VideoFrame {
  width: number;
  height: number;
  data: Uint8Array;
}

// Shape returned directly by the `get_video_frame` Tauri command: `data` is
// a base64 string (see src-tauri's VideoFrame custom serializer), not a raw
// JSON number array -- serde_json has no compact byte representation, so a
// plain `Vec<u8>` would otherwise serialize a ~230KB frame as `[255,0,...]`
// polled up to 60x/sec. Decoded into a Uint8Array immediately below.
interface RawVideoFrame {
  width: number;
  height: number;
  data: string;
}

// Decodes a base64 string (as produced by the Rust-side VideoFrame's custom
// serializer) into a Uint8Array of raw bytes.
function base64ToUint8Array(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export interface InputState {
  buttons: number;
  x: number;
  y: number;
}

interface EmulationState {
  isRunning: boolean;
  isPaused: boolean;
  currentGame: GameInfo | null;
  frameRate: number;
  frame: VideoFrame | null;
  /** Interleaved stereo PCM samples: L0, R0, L1, R1, ... (see `getFrame`). */
  audioBuffer: number[];
  
  // Actions
  loadRom: (path: string) => Promise<void>;
  start: (gameId?: string) => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  stop: () => Promise<void>;
  setInput: (input: InputState) => Promise<void>;
  getFrame: () => Promise<void>;
  saveState: (slot: number) => Promise<void>;
  loadState: (slot: number) => Promise<void>;
}

// Guards against out-of-order start/pause/resume/stop calls: if a newer call
// has been issued before an older one's invoke() resolves, the older call's
// result is discarded instead of clobbering state set by the newer call.
let opSeq = 0;

export const useEmulationStore = create<EmulationState>((set) => ({
  isRunning: false,
  isPaused: false,
  currentGame: null,
  frameRate: 60,
  frame: null,
  audioBuffer: [],

  loadRom: async (path: string) => {
    try {
      const gameInfo = await invoke<GameInfo>('load_rom', { path });
      set({ currentGame: gameInfo });
    } catch (error) {
      console.error('Failed to load ROM:', error);
      throw error;
    }
  },

  start: async (gameId?: string) => {
    const seq = ++opSeq;
    try {
      await invoke('start_emulation', { gameId: gameId ?? null });
      if (seq === opSeq) set({ isRunning: true, isPaused: false });
    } catch (error) {
      console.error('Failed to start emulation:', error);
      throw error;
    }
  },

  pause: async () => {
    const seq = ++opSeq;
    try {
      await invoke('pause_emulation');
      if (seq === opSeq) set({ isPaused: true });
    } catch (error) {
      console.error('Failed to pause emulation:', error);
      throw error;
    }
  },

  resume: async () => {
    const seq = ++opSeq;
    try {
      await invoke('resume_emulation');
      if (seq === opSeq) set({ isPaused: false });
    } catch (error) {
      console.error('Failed to resume emulation:', error);
      throw error;
    }
  },

  stop: async () => {
    const seq = ++opSeq;
    try {
      await invoke('stop_emulation');
      if (seq === opSeq) set({ isRunning: false, isPaused: false });
    } catch (error) {
      console.error('Failed to stop emulation:', error);
      throw error;
    }
  },

  setInput: async (input: InputState) => {
    try {
      await invoke('set_input_state', { input });
    } catch (error) {
      console.error('Failed to set input:', error);
    }
  },

  getFrame: async () => {
    const seq = ++opSeq;
    try {
      const raw = await invoke<RawVideoFrame>('get_video_frame');
      // Interleaved stereo PCM (L0, R0, L1, R1, ...) -- see
      // `Snes::get_audio_samples`/`EmulationController::get_audio` on the
      // Rust side, which now drain the DSP's real per-voice-panned L/R
      // output instead of an averaged-to-mono value. `count` is accepted
      // by convention but the backend command itself takes no arguments;
      // it just drains whatever `step_frame()` already buffered.
      const audio = await invoke<number[]>('get_audio_samples', { count: 2048 });
      // Discard stale responses: if a newer getFrame (or start/pause/resume/
      // stop) call has been issued before this one's invokes resolved,
      // applying this result would clobber state with an older frame.
      if (seq !== opSeq) return;
      const frame: VideoFrame = {
        width: raw.width,
        height: raw.height,
        data: base64ToUint8Array(raw.data),
      };
      set({ frame, audioBuffer: audio });
    } catch (error) {
      console.error('Failed to get frame:', error);
    }
  },

  saveState: async (slot: number) => {
    try {
      await invoke('save_state', { slot });
    } catch (error) {
      console.error('Failed to save state:', error);
      throw error;
    }
  },

  loadState: async (slot: number) => {
    try {
      await invoke('load_state', { slot });
    } catch (error) {
      console.error('Failed to load state:', error);
      throw error;
    }
  },
}));
