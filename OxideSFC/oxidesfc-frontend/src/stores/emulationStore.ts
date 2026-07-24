import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { getAudioService } from '../services/audio';

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
  audioBuffer: Int16Array;
  
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

/// Shared empty buffer used to mark "nothing new to play", so the render loop
/// never re-queues a previous poll's samples. See `getFrame`.
const EMPTY_AUDIO = new Int16Array(0);

export const useEmulationStore = create<EmulationState>((set) => ({
  isRunning: false,
  isPaused: false,
  currentGame: null,
  frameRate: 60,
  frame: null,
  audioBuffer: EMPTY_AUDIO,

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
      // `null` means no new emulated frame completed since the last poll
      // (the caller runs at monitor refresh rate, the console at ~60fps)
      // -- the previous frame is still current, skip the ~230KB base64
      // decode and keep rendering it.
      const raw = await invoke<RawVideoFrame | null>('get_video_frame');
      // Interleaved stereo PCM (L0, R0, L1, R1, ...) -- see
      // `Snes::get_audio_samples`/`EmulationController::get_audio` on the
      // Rust side. The command returns raw little-endian i16 bytes over
      // Tauri's binary IPC path (`tauri::ipc::Response`), which arrives
      // here as an ArrayBuffer -- no JSON number-array encode/parse per
      // frame -- and is viewed as an Int16Array for free.
      const audioBytes = await invoke<ArrayBuffer>('get_audio_samples');
      // Discard stale responses: if a newer getFrame (or start/pause/resume/
      // stop) call has been issued before this one's invokes resolved,
      // applying this result would clobber state with an older frame.
      //
      // The stored audioBuffer has to be cleared on the way out, though. The
      // render loop queues whatever it finds in the store, so leaving the
      // previous call's buffer in place made that audio play a SECOND time --
      // an audible repeated ~16ms chunk on every superseded poll, on top of
      // the gap left by this response's (already drained, now discarded)
      // samples. Same reasoning on the error path below.
      if (seq !== opSeq) {
        set({ audioBuffer: EMPTY_AUDIO });
        return;
      }
      const audioBuffer = new Int16Array(audioBytes);
      if (raw) {
        const frame: VideoFrame = {
          width: raw.width,
          height: raw.height,
          data: base64ToUint8Array(raw.data),
        };
        set({ frame, audioBuffer });
      } else {
        set({ audioBuffer });
      }
    } catch (error) {
      console.error('Failed to get frame:', error);
      set({ audioBuffer: EMPTY_AUDIO });
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
      // The emulated timeline just jumped: anything still queued on the
      // audio thread is pre-jump audio (the Rust side clears its own
      // buffers too). Discarding instead of stopping keeps playback
      // primed for the post-load samples.
      getAudioService().clear();
    } catch (error) {
      console.error('Failed to load state:', error);
      throw error;
    }
  },
}));
