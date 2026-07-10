import { useEffect, useRef, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useEmulationStore } from '../../stores/emulationStore';
import { useSettingsStore } from '../../stores/settingsStore';
import { WebGLRenderer } from '../../services/renderer';
import { getAudioService } from '../../services/audio';
import { useGamepad } from '../../hooks/useGamepad';
import { QuickMenu } from './QuickMenu';

interface EmulatorViewProps {
  onExit: () => void;
}

export function EmulatorView({ onExit }: EmulatorViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<WebGLRenderer | null>(null);
  const animationRef = useRef<number | null>(null);
  const initializedRef = useRef(false);
  const audioServiceRef = useRef<ReturnType<typeof getAudioService> | null>(null);
  
  const { settings } = useSettingsStore();
  const { 
    isRunning,
    isPaused,
    currentGame,
    frame,
    pause,
    resume,
    stop,
    getFrame,
    setInput,
  } = useEmulationStore();
  
  const theme = settings.general?.theme || 'dark';
  const [showMenu, setShowMenu] = useState(false);
  const [webglStatus, setWebglStatus] = useState<string>('');
  const [audioStatus, setAudioStatus] = useState<string>('');
  // Emulation speed multiplier (1.00 = real NTSC speed). Applied on the
  // backend (wall-clock frame pacing) AND to the audio service's playback
  // rate, so pitch/tempo follow the game speed like a real console would.
  const [speed, setSpeed] = useState(1.0);

  const applySpeed = useCallback(async (value: number) => {
    const requested = Math.round(Math.max(0.1, Math.min(4, value)) * 100) / 100;
    try {
      const applied = await invoke<number>('set_emulation_speed', { speed: requested });
      setSpeed(applied);
      audioServiceRef.current?.setPlaybackRate(applied);
    } catch (error) {
      console.error('Failed to set emulation speed:', error);
    }
  }, []);

  // Pick up whatever speed the backend already has (it persists across
  // view unmounts within a session) and mirror it into the audio service.
  useEffect(() => {
    invoke<number>('get_emulation_speed')
      .then((value) => {
        setSpeed(value);
        audioServiceRef.current?.setPlaybackRate(value);
      })
      .catch(() => {});
  }, []);

  // Input mapping
  //
  // Hardcoded default map, used as a fallback for any SNES button that has
  // no entry in the user's saved `settings.controls.keyboard_mapping` (e.g.
  // a fresh install, or a mapping object missing a key). The *actual*
  // key-to-button lookup used at key-event time is `keyToButtonRef` below,
  // which is rebuilt from the live settings so in-game remapping (done via
  // ControllerSettings.tsx, which persists key-code -> SNES-button-name
  // pairs into `settings.controls.keyboard_mapping`) takes effect during
  // play instead of being silently ignored.
  const DEFAULT_KEY_TO_BUTTON: Record<string, number> = {
    'ArrowUp': 0x01,
    'ArrowDown': 0x02,
    'ArrowLeft': 0x04,
    'ArrowRight': 0x08,
    'KeyZ': 0x10, // A
    'KeyX': 0x20, // B
    'Enter': 0x40, // Start
    'ShiftRight': 0x80, // Select
    'KeyA': 0x100, // L
    'KeyS': 0x200, // R
  };

  // SNES button name (as stored in keyboard_mapping values) -> bitmask.
  const BUTTON_NAME_TO_MASK: Record<string, number> = {
    up: 0x01,
    down: 0x02,
    left: 0x04,
    right: 0x08,
    a: 0x10,
    b: 0x20,
    start: 0x40,
    select: 0x80,
    l: 0x100,
    r: 0x200,
    x: 0x400,
    y: 0x800,
  };

  // Rebuilt whenever the persisted keyboard_mapping changes, so remapping
  // takes effect immediately without needing to remount this component.
  // Falls back to the hardcoded default map for any key that has no custom
  // binding.
  const keyToButtonRef = useRef<Record<string, number>>(DEFAULT_KEY_TO_BUTTON);

  useEffect(() => {
    const customMapping = settings.controls?.keyboard_mapping;
    if (!customMapping || Object.keys(customMapping).length === 0) {
      keyToButtonRef.current = DEFAULT_KEY_TO_BUTTON;
      return;
    }

    const merged: Record<string, number> = { ...DEFAULT_KEY_TO_BUTTON };
    for (const [keyCode, buttonName] of Object.entries(customMapping)) {
      const mask = BUTTON_NAME_TO_MASK[buttonName];
      if (mask !== undefined) {
        merged[keyCode] = mask;
      }
    }
    keyToButtonRef.current = merged;
  }, [settings.controls?.keyboard_mapping]);

  const pressedKeys = useRef<Set<string>>(new Set());

  // Gamepad input, polled via requestAnimationFrame while this view is
  // mounted (see useGamepad's internal setInterval polling of
  // navigator.getGamepads()). Combined with the keyboard state below so
  // either input source pressing a button registers the press.
  //
  // NOTE on bit layouts: useGamepad.ts's own internal BUTTON_MASK constant
  // (used only inside that hook to build its `pressedButtons` Set) uses a
  // *different* bit assignment than the `buttons` field the Rust backend
  // expects (see EmulationController::set_controller_input in
  // src-tauri/src/emulation/controller.rs, which hardcodes the same layout
  // as this file's DEFAULT_KEY_TO_BUTTON/BUTTON_NAME_TO_MASK below -- e.g.
  // Up=0x01, A=0x10, Start=0x40). Calling useGamepad's getInputState() and
  // OR-ing its raw `buttons` number directly into ours would silently set
  // the wrong bits. So instead we read its already-decoded
  // `getPressedButtons()` (an InputButton[] of domain names like 'up'/'a',
  // the same vocabulary keyboard_mapping values use) and translate through
  // BUTTON_NAME_TO_MASK -- the single source of truth for the wire format.
  const gamepadEnabled = settings.controls?.gamepad_enabled ?? true;
  const { getPressedButtons } = useGamepad({ enabled: gamepadEnabled });

  // useGamepad's getPressedButtons identity changes on every poll tick
  // (it closes over a `pressedButtons` useState that updates ~60x/sec while
  // a pad is connected). Depending on it directly in the rAF effect below
  // would tear down and rebuild that effect every tick instead of running
  // one stable loop, so it's mirrored into a ref here (updated on every
  // render, not inside an effect) and the polling effect reads
  // getPressedButtonsRef.current instead of taking getPressedButtons as a
  // dependency.
  const getPressedButtonsRef = useRef(getPressedButtons);
  getPressedButtonsRef.current = getPressedButtons;

  const gamepadInputRef = useRef<{ buttons: number; x: number; y: number }>({ buttons: 0, x: 0, y: 0 });

  // Recompute combined (keyboard | gamepad) input and push it to the
  // emulator via the same setInput path both sources share.
  const publishCombinedInput = useCallback(() => {
    // Calculate keyboard button state
    let buttons = 0;
    for (const key of pressedKeys.current) {
      const mask = keyToButtonRef.current[key];
      if (mask) {
        buttons |= mask;
      }
    }

    // Determine D-pad direction from keyboard
    let x = 0;
    let y = 0;
    if (pressedKeys.current.has('ArrowLeft')) x = -1;
    if (pressedKeys.current.has('ArrowRight')) x = 1;
    if (pressedKeys.current.has('ArrowUp')) y = -1;
    if (pressedKeys.current.has('ArrowDown')) y = 1;

    // Compose with gamepad state -- either source pressing a button
    // registers the press; last non-zero axis value wins for x/y.
    const gp = gamepadInputRef.current;
    buttons |= gp.buttons;
    if (x === 0 && gp.x !== 0) x = gp.x;
    if (y === 0 && gp.y !== 0) y = gp.y;

    setInput({ buttons, x, y });
  }, [setInput]);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (pressedKeys.current.has(e.code)) return;
    pressedKeys.current.add(e.code);
    publishCombinedInput();
  }, [publishCombinedInput]);

  const handleKeyUp = useCallback((e: KeyboardEvent) => {
    pressedKeys.current.delete(e.code);
    publishCombinedInput();
  }, [publishCombinedInput]);

  // Initialize WebGL renderer
  //
  // React 18 StrictMode double-invokes effects in dev: mount -> cleanup ->
  // mount. Because renderer construction is async (`renderer.initialize()`
  // awaits GPU context/shader setup), the *first* invocation's cleanup can
  // run and complete before its `await` resolves -- at which point
  // `initializedRef.current` is still false (it was only ever set to true
  // *after* the await), so the guard at the top of this effect does nothing
  // to stop the second invocation from starting a second, independent
  // WebGLRenderer construction. Without a per-invocation cancellation
  // token, the first renderer's async init eventually resolves, assigns
  // itself to `rendererRef.current`, and orphans its WebGL context/GPU
  // resources when the second (superseding) renderer later overwrites that
  // ref -- WebGL contexts are a finite per-page resource, so repeated
  // remounts eventually exhaust them ("too many active WebGL contexts").
  //
  // Fix: each invocation gets its own `cancelled` flag captured by closure.
  // If this specific invocation is cancelled (its cleanup ran) by the time
  // its `await renderer.initialize()` resolves, it disposes the renderer it
  // just built instead of publishing it to `rendererRef`/state, so at most
  // one renderer is ever live for this component instance.
  useEffect(() => {
    if (!canvasRef.current) return;

    let cancelled = false;

    const initRenderer = async () => {
      if (!canvasRef.current) return;

      const renderer = new WebGLRenderer(canvasRef.current, {
        width: 512,
        height: 480,
        scaleMode: (settings.video?.scale_mode as 'nearest' | 'bilinear' | 'xbrz' | 'hq2x') || 'nearest',
        crtMode: false,
      });

      const success = await renderer.initialize();

      if (cancelled) {
        // Superseded by a later invocation (e.g. StrictMode's second
        // mount) while this construction was in flight -- clean up this
        // renderer's GPU resources instead of publishing it.
        renderer.dispose();
        return;
      }

      if (success) {
        rendererRef.current = renderer;
        initializedRef.current = true;
        setWebglStatus(`WebGL: ${renderer.getWebGLVersion()}`);
        console.log('WebGL renderer initialized successfully');
      } else {
        setWebglStatus('WebGL: Failed to initialize');
        console.error('Failed to initialize WebGL renderer');
      }
    };

    initRenderer();

    return () => {
      cancelled = true;
      if (rendererRef.current) {
        rendererRef.current.dispose();
        rendererRef.current = null;
        initializedRef.current = false;
      }
    };
  }, []);

  // Update renderer options when settings change
  useEffect(() => {
    if (!rendererRef.current) return;
    
    rendererRef.current.setOptions({
      scaleMode: (settings.video?.scale_mode as 'nearest' | 'bilinear' | 'xbrz' | 'hq2x') || 'nearest',
      crtMode: settings.video?.shader === 'crt',
    });
  }, [settings.video?.scale_mode, settings.video?.shader]);

  // Initialize audio service
  useEffect(() => {
    const initAudio = async () => {
      const audioService = getAudioService({
        // Match the SNES DSP output rate so samples play at the correct
        // pitch with matched produce/consume rates (see AudioService).
        sampleRate: 32000,
        latency: settings.audio?.latency || 50,
        channels: 'stereo',
      });
      
      const success = await audioService.initialize();
      
      if (success) {
        // Set volume from settings
        const volume = settings.audio?.volume ?? 1.0;
        audioService.setVolume(volume * 100);
        
        // Set mute if disabled
        if (settings.audio?.enabled === false) {
          audioService.setMuted(true);
        }
        
        // Set audio source callback to fetch samples from emulation
        audioService.setAudioSource(async (_count: number) => {
          // This will be called by the audio service to get samples
          // We'll use the audioBuffer from the store instead
          return []; // Will be overridden by queueAudio in render loop
        });
        
        audioServiceRef.current = audioService;
        setAudioStatus(`Audio: ${audioService.getSampleRate()}Hz`);
        console.log('AudioService initialized successfully');
        // Begin playback now. Audio init is async and usually completes
        // AFTER the render-loop effect has already run its one-shot
        // `start()` call (when this ref was still null), so without this
        // the service would sit un-started -- `isPlaying` false, and
        // `queueAudio` silently drops every sample. Starting here (the
        // emulator view only mounts to play) guarantees playback begins.
        audioService.start();
      } else {
        setAudioStatus('Audio: Failed to initialize');
        console.error('Failed to initialize AudioService');
      }
    };
    
    initAudio();
    
    return () => {
      if (audioServiceRef.current) {
        audioServiceRef.current.dispose();
        audioServiceRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);

    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('keyup', handleKeyUp);
    };
  }, [handleKeyDown, handleKeyUp]);

  // Gamepad polling loop.
  //
  // useGamepad() (above) already polls navigator.getGamepads() on its own
  // setInterval and exposes the latest pressed buttons via
  // getPressedButtons(). This effect just samples that state on every
  // animation frame while the view is mounted and gameplay is running,
  // translates it into the wire-format bitmask + x/y pair via
  // BUTTON_NAME_TO_MASK, stores it in gamepadInputRef, and republishes the
  // combined (keyboard | gamepad) input -- so a gamepad press/release is
  // reflected even if no keyboard event fires in between.
  useEffect(() => {
    if (!isRunning || !gamepadEnabled) return;

    let cancelled = false;
    let rafId: number;

    const pollGamepad = () => {
      if (cancelled) return;

      const pressed = getPressedButtonsRef.current();
      let buttons = 0;
      let x = 0;
      let y = 0;
      for (const button of pressed) {
        const mask = BUTTON_NAME_TO_MASK[button];
        if (mask) buttons |= mask;
      }
      if (pressed.includes('left')) x = -1;
      if (pressed.includes('right')) x = 1;
      if (pressed.includes('up')) y = -1;
      if (pressed.includes('down')) y = 1;

      const prev = gamepadInputRef.current;
      if (buttons !== prev.buttons || x !== prev.x || y !== prev.y) {
        gamepadInputRef.current = { buttons, x, y };
        publishCombinedInput();
      }

      rafId = requestAnimationFrame(pollGamepad);
    };

    rafId = requestAnimationFrame(pollGamepad);

    return () => {
      cancelled = true;
      cancelAnimationFrame(rafId);
    };
  }, [isRunning, gamepadEnabled, publishCombinedInput]);

  // Render loop
  //
  // `frame`/`audioBuffer` must NOT be in this effect's dependency array:
  // getFrame() below updates them on every call, which would retrigger this
  // same effect, cancel the in-flight requestAnimationFrame, and immediately
  // fire a new getFrame() -- an uncontrolled cascade of overlapping
  // get_video_frame invocations all serializing behind the backend's single
  // EmulationController mutex, which made the emulator appear completely
  // frozen. Reading the freshly-set values via getState() right after
  // getFrame() resolves also avoids rendering a stale (one-fetch-behind)
  // frame from the effect's original closure.
  useEffect(() => {
    if (!isRunning) return;

    // Start audio playback when emulation starts
    if (audioServiceRef.current) {
      audioServiceRef.current.start();
    }

    let cancelled = false;

    const render = async () => {
      if (cancelled) return;

      try {
        await getFrame();
        const { frame: latestFrame, audioBuffer: latestAudioBuffer } = useEmulationStore.getState();

        // Render frame using WebGL
        if (rendererRef.current && latestFrame && latestFrame.data) {
          // Convert frame data to Uint8Array if needed
          const data = latestFrame.data instanceof Uint8Array
            ? latestFrame.data
            : new Uint8Array(latestFrame.data);

          rendererRef.current.render(data, latestFrame.width, latestFrame.height);
        }

        // Queue audio samples for playback
        if (audioServiceRef.current && latestAudioBuffer && latestAudioBuffer.length > 0) {
          audioServiceRef.current.queueAudio(latestAudioBuffer);
        }
      } catch (error) {
        // Without this, a thrown error here (e.g. a WebGL error mid-render)
        // becomes an unhandled promise rejection that silently kills the
        // loop after one iteration -- requestAnimationFrame(render) below
        // never gets scheduled again, and unhandled rejections aren't
        // caught by console.* patching, so the emulator just appears
        // "frozen" with zero visible errors.
        console.error('Render loop error:', error);
      }

      if (!cancelled) {
        animationRef.current = requestAnimationFrame(render);
      }
    };

    render();

    return () => {
      cancelled = true;
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current);
      }
    };
  }, [isRunning, getFrame]);

  const handlePauseResume = async () => {
    if (isPaused) {
      await resume();
    } else {
      await pause();
    }
  };

  const handleStop = async () => {
    await stop();
    onExit();
  };

  if (!currentGame) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-lg">No game loaded</div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-black">
      {/* Emulator Canvas */}
      <div className="flex-1 flex items-center justify-center">
        <canvas
          ref={canvasRef}
          className="emulator-canvas max-w-full max-h-full"
          style={{ width: '100%', height: '100%', objectFit: 'contain' }}
        />
      </div>

      {/* Status Bar */}
      <div className={`flex items-center justify-between px-4 py-1 text-xs ${theme === 'light' ? 'bg-gray-100 text-gray-600' : 'bg-slate-900 text-gray-400'}`}>
        <div className="flex items-center gap-4">
          <span>{webglStatus}</span>
          <span>{audioStatus}</span>
        </div>
        <span>{frame ? `${frame.width}x${frame.height}` : 'No signal'}</span>
      </div>

      {/* Controls Bar */}
      <div className={`flex items-center justify-between px-4 py-2 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
        <div className="flex items-center gap-4">
          <span className="font-semibold">{currentGame.title}</span>
          <span className="text-sm text-gray-400">
            {isPaused ? 'PAUSED' : 'RUNNING'}
          </span>
        </div>

        {/* Speed control: -/+ in 0.05x steps, click the value to reset to
            1.00x. Backend pacing and audio playback rate move together. */}
        <div className="flex items-center gap-1 text-sm">
          <span className="text-gray-400 mr-1">Speed</span>
          <button
            onClick={() => applySpeed(speed - 0.05)}
            className="px-2 py-1 bg-slate-600 hover:bg-slate-500 rounded"
            title="Slower (-0.05x)"
          >
            −
          </button>
          <button
            onClick={() => applySpeed(1.0)}
            className={`px-2 py-1 rounded font-mono min-w-[4.5rem] text-center ${speed === 1.0 ? 'text-gray-300' : 'text-yellow-400'}`}
            title="Reset to 1.00x"
          >
            {speed.toFixed(2)}x
          </button>
          <button
            onClick={() => applySpeed(speed + 0.05)}
            className="px-2 py-1 bg-slate-600 hover:bg-slate-500 rounded"
            title="Faster (+0.05x)"
          >
            +
          </button>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={handlePauseResume}
            className="px-4 py-2 bg-primary-600 hover:bg-primary-700 rounded text-sm"
          >
            {isPaused ? 'Resume' : 'Pause'}
          </button>
          
          <button
            onClick={() => setShowMenu(!showMenu)}
            className="px-4 py-2 bg-slate-600 hover:bg-slate-500 rounded text-sm"
          >
            Menu
          </button>
          
          <button
            onClick={handleStop}
            className="px-4 py-2 bg-red-600 hover:bg-red-700 rounded text-sm"
          >
            Exit
          </button>
        </div>
      </div>

      {/* Menu Overlay */}
      <QuickMenu
        isOpen={showMenu}
        onClose={() => setShowMenu(false)}
        onOpenSettings={() => setShowMenu(false)}
        onExitToMenu={onExit}
        theme={theme === 'light' ? 'light' : 'dark'}
        canvasRef={canvasRef}
        gameTitle={currentGame.title}
      />
    </div>
  );
}
