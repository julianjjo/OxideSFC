import { useEffect, useRef, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEmulationStore } from '../../stores/emulationStore';
import { useSettingsStore } from '../../stores/settingsStore';
import { WebGLRenderer } from '../../services/renderer';
import { getAudioService } from '../../services/audio';
import { useGamepad } from '../../hooks/useGamepad';
import { DEFAULT_KEYBOARD_MAPPING, SNES_BUTTON_BITMASK } from '../../domain/keyboardDefaults';
import { QuickMenu } from './QuickMenu';
import { ControlDeck } from './ControlDeck';
import { captureScreenshot } from './captureScreenshot';

interface EmulatorViewProps {
  onExit: () => void;
  onOpenSettings: () => void;
}

/** Idle time (no mouse activity) before the control deck hides during play. */
const CONTROLS_HIDE_DELAY_MS = 2500;

export function EmulatorView({ onExit, onOpenSettings }: EmulatorViewProps) {
  const stageRef = useRef<HTMLDivElement>(null);
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
    saveState,
    loadState,
  } = useEmulationStore();

  // No theme lookup here: the play view is a black stage in both themes (a light
  // chrome floating over a game image would glare), and the deck and quick menu
  // resolve their own colours from tokens.
  const [showMenu, setShowMenu] = useState(false);
  const [webglStatus, setWebglStatus] = useState<string>('');
  const [audioStatus, setAudioStatus] = useState<string>('');
  const [isFullscreen, setIsFullscreen] = useState(false);
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

  // -------------------------------------------------------------------------
  // Canvas sizing
  //
  // The canvas is absolutely positioned inside the full-bleed stage and sized
  // entirely from here: letterboxed to the current frame's aspect ratio via a
  // ResizeObserver on the stage, with the drawing buffer scaled by
  // devicePixelRatio for crisp output on scaled displays. The renderer never
  // touches canvas.width/height (it used to derive them from parentElement
  // inside the render loop, which fed the buffer's intrinsic size back into
  // flex layout and pushed the UI off-screen until a window resize forced a
  // clean re-layout -- the "game is cut off until I resize" bug).
  // -------------------------------------------------------------------------
  const frameSizeRef = useRef({ width: 256, height: 224 });

  const layoutCanvas = useCallback(() => {
    const stage = stageRef.current;
    const canvas = canvasRef.current;
    if (!stage || !canvas) return;

    const stageWidth = stage.clientWidth;
    const stageHeight = stage.clientHeight;
    if (stageWidth === 0 || stageHeight === 0) return;

    const { width: frameWidth, height: frameHeight } = frameSizeRef.current;
    const aspect = frameWidth / frameHeight;

    let cssWidth = stageWidth;
    let cssHeight = stageHeight;
    if (stageWidth / stageHeight > aspect) {
      cssWidth = stageHeight * aspect;
    } else {
      cssHeight = stageWidth / aspect;
    }
    cssWidth = Math.floor(cssWidth);
    cssHeight = Math.floor(cssHeight);

    canvas.style.width = `${cssWidth}px`;
    canvas.style.height = `${cssHeight}px`;
    canvas.style.left = `${Math.floor((stageWidth - cssWidth) / 2)}px`;
    canvas.style.top = `${Math.floor((stageHeight - cssHeight) / 2)}px`;

    const dpr = window.devicePixelRatio || 1;
    const bufferWidth = Math.max(1, Math.round(cssWidth * dpr));
    const bufferHeight = Math.max(1, Math.round(cssHeight * dpr));
    if (canvas.width !== bufferWidth || canvas.height !== bufferHeight) {
      canvas.width = bufferWidth;
      canvas.height = bufferHeight;
    }
  }, []);

  useEffect(() => {
    layoutCanvas();
    const stage = stageRef.current;
    if (!stage) return;
    const observer = new ResizeObserver(() => layoutCanvas());
    observer.observe(stage);
    return () => observer.disconnect();
  }, [layoutCanvas]);

  // -------------------------------------------------------------------------
  // Transient feedback (toast) for save/load/screenshot actions
  // -------------------------------------------------------------------------
  const [toast, setToast] = useState<{ id: number; text: string; tone: 'ok' | 'err' } | null>(null);
  const toastIdRef = useRef(0);

  const showToast = useCallback((text: string, tone: 'ok' | 'err' = 'ok') => {
    const id = ++toastIdRef.current;
    setToast({ id, text, tone });
    window.setTimeout(() => {
      setToast((current) => (current && current.id === id ? null : current));
    }, 2200);
  }, []);

  // -------------------------------------------------------------------------
  // Control deck auto-hide
  //
  // The deck (and the pointer) hides after a short idle period while the game
  // is actually playing; any mouse activity, pausing, opening the menu, or
  // keyboard focus inside the deck brings it back / keeps it up.
  // -------------------------------------------------------------------------
  const [controlsVisible, setControlsVisible] = useState(true);
  const hideTimerRef = useRef<number | null>(null);
  // Live mirror of the state the hide-timer callback needs, so the timeout
  // never acts on a stale closure.
  const uiStateRef = useRef({ isRunning, isPaused, showMenu, deckActive: false });
  uiStateRef.current.isRunning = isRunning;
  uiStateRef.current.isPaused = isPaused;
  uiStateRef.current.showMenu = showMenu;

  const clearHideTimer = useCallback(() => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
  }, []);

  const scheduleHide = useCallback(() => {
    clearHideTimer();
    hideTimerRef.current = window.setTimeout(() => {
      const s = uiStateRef.current;
      if (s.isRunning && !s.isPaused && !s.showMenu && !s.deckActive) {
        setControlsVisible(false);
      }
    }, CONTROLS_HIDE_DELAY_MS);
  }, [clearHideTimer]);

  const showControls = useCallback(() => {
    setControlsVisible(true);
    scheduleHide();
  }, [scheduleHide]);

  // Pin the controls whenever play is interrupted; restart the idle timer
  // when gameplay resumes.
  useEffect(() => {
    if (!isRunning || isPaused || showMenu) {
      clearHideTimer();
      setControlsVisible(true);
    } else {
      scheduleHide();
    }
  }, [isRunning, isPaused, showMenu, clearHideTimer, scheduleHide]);

  useEffect(() => clearHideTimer, [clearHideTimer]);

  const handleDeckActive = useCallback(
    (active: boolean) => {
      uiStateRef.current.deckActive = active;
      if (active) {
        clearHideTimer();
        setControlsVisible(true);
      } else {
        scheduleHide();
      }
    },
    [clearHideTimer, scheduleHide]
  );

  // Input mapping
  //
  // DEFAULT_KEY_TO_BUTTON (a fallback for any SNES button that has no entry
  // in the user's saved `settings.controls.keyboard_mapping` -- e.g. a fresh
  // install, or a mapping object missing a key) and BUTTON_NAME_TO_MASK (the
  // SNES button name -> wire-format bitmask lookup) are both derived from
  // DEFAULT_KEYBOARD_MAPPING/SNES_BUTTON_BITMASK in domain/keyboardDefaults.ts
  // -- the single source of truth shared with ControllerSettings.tsx, so the
  // two can't drift out of sync with each other again. The *actual*
  // key-to-button lookup used at key-event time is `keyToButtonRef` below,
  // which is rebuilt from the live settings so in-game remapping (done via
  // ControllerSettings.tsx, which persists key-code -> SNES-button-name
  // pairs into `settings.controls.keyboard_mapping`) takes effect during
  // play instead of being silently ignored.
  const BUTTON_NAME_TO_MASK: Record<string, number> = SNES_BUTTON_BITMASK;
  const DEFAULT_KEY_TO_BUTTON: Record<string, number> = Object.fromEntries(
    Object.entries(DEFAULT_KEYBOARD_MAPPING).map(([keyCode, button]) => [keyCode, BUTTON_NAME_TO_MASK[button]])
  );

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
  // The deadzone has to be forwarded, not left to the hook's own default:
  // `settings.controls.gamepad_deadzone` was persisted and given a slider while
  // nothing read it, so raising it to cure stick drift changed nothing --
  // `useGamepad` fell back to a hardcoded 0.15 every time.
  const gamepadDeadzone = settings.controls?.gamepad_deadzone ?? 0.1;
  const { getPressedButtons } = useGamepad({
    enabled: gamepadEnabled,
    deadzone: gamepadDeadzone,
  });

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
        setWebglStatus(renderer.getWebGLVersion());
        console.log('WebGL renderer initialized successfully');
      } else {
        setWebglStatus('no WebGL');
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
        latency: settings.audio?.latency || 60,
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

        audioServiceRef.current = audioService;
        setAudioStatus(`${(audioService.getSampleRate() / 1000).toFixed(0)} kHz`);
        console.log('AudioService initialized successfully');
        // Begin playback now. Audio init is async and usually completes
        // AFTER the render-loop effect has already run its one-shot
        // `start()` call (when this ref was still null), so without this
        // the service would sit un-started -- `isPlaying` false, and
        // `queueAudio` silently drops every sample. Starting here (the
        // emulator view only mounts to play) guarantees playback begins.
        audioService.start();
      } else {
        setAudioStatus('no audio');
        console.error('Failed to initialize AudioService');
      }
    };

    initAudio();

    return () => {
      // Stop playback but keep the singleton service (and its
      // AudioContext) alive across mounts. Disposing here raced React 18
      // StrictMode's double effect invocation: the first cleanup tore the
      // service down while the second run's async initialize() was still
      // mid-addModule, so the worklet node was constructed on a context
      // with no registered processor. The next mount just reuses the
      // already-initialized service.
      if (audioServiceRef.current) {
        audioServiceRef.current.stop();
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
          // Re-letterbox if the console changed output resolution (e.g.
          // switching into a hi-res or interlaced mode).
          if (
            latestFrame.width !== frameSizeRef.current.width ||
            latestFrame.height !== frameSizeRef.current.height
          ) {
            frameSizeRef.current = { width: latestFrame.width, height: latestFrame.height };
            layoutCanvas();
          }

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
  }, [isRunning, getFrame, layoutCanvas]);

  // -------------------------------------------------------------------------
  // Deck / hotkey actions
  // -------------------------------------------------------------------------
  const handlePauseResume = useCallback(async () => {
    if (isPaused) {
      await resume();
    } else {
      await pause();
    }
  }, [isPaused, pause, resume]);

  const toggleFullscreen = useCallback(async () => {
    try {
      const appWindow = getCurrentWindow();
      const next = !(await appWindow.isFullscreen());
      await appWindow.setFullscreen(next);
      setIsFullscreen(next);
    } catch (error) {
      console.error('Failed to toggle fullscreen:', error);
    }
  }, []);

  // The window keeps its fullscreen state across view remounts (e.g. a trip
  // to Settings and back), so sync rather than assuming windowed.
  useEffect(() => {
    getCurrentWindow()
      .isFullscreen()
      .then(setIsFullscreen)
      .catch(() => {});
  }, []);

  const handleStop = useCallback(async () => {
    // Leave fullscreen when returning to the library so the user isn't
    // stranded in a chromeless window.
    try {
      const appWindow = getCurrentWindow();
      if (await appWindow.isFullscreen()) {
        await appWindow.setFullscreen(false);
      }
    } catch {
      // Fullscreen state is cosmetic here; exiting the game matters more.
    }
    await stop();
    onExit();
  }, [stop, onExit]);

  const handleQuickSave = useCallback(async () => {
    try {
      await saveState(0);
      showToast('State saved to slot 1');
    } catch {
      showToast('Save failed', 'err');
    }
  }, [saveState, showToast]);

  const handleQuickLoad = useCallback(async () => {
    try {
      await loadState(0);
      showToast('State loaded from slot 1');
    } catch {
      showToast('Load failed', 'err');
    }
  }, [loadState, showToast]);

  const handleScreenshot = useCallback(async () => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    try {
      const result = await captureScreenshot(canvas, currentGame?.title);
      if (result === 'saved') {
        showToast('Screenshot saved');
      }
    } catch (error) {
      console.error('Failed to take screenshot:', error);
      showToast('Screenshot failed', 'err');
    }
  }, [currentGame?.title, showToast]);

  // Global gameplay hotkeys. Keys the user has mapped to SNES buttons always
  // win (so a custom mapping of Space to a game button disables the pause
  // hotkey rather than fighting it), and the quick menu owns the keyboard
  // while it is open (it has its own handler, including Escape-to-close).
  useEffect(() => {
    const onHotkey = (e: KeyboardEvent) => {
      if (showMenu) return;
      // Holding a key must not machine-gun the action (Space auto-repeat
      // would toggle pause dozens of times per second).
      if (e.repeat) return;
      if (keyToButtonRef.current[e.code]) return;

      switch (e.code) {
        case 'Space':
          e.preventDefault();
          handlePauseResume();
          break;
        case 'Escape':
          e.preventDefault();
          setShowMenu(true);
          break;
        case 'F5':
          e.preventDefault();
          handleQuickSave();
          break;
        case 'F9':
          e.preventDefault();
          handleQuickLoad();
          break;
        case 'F8':
          e.preventDefault();
          handleScreenshot();
          break;
        case 'F11':
          e.preventDefault();
          toggleFullscreen();
          break;
      }
    };

    window.addEventListener('keydown', onHotkey);
    return () => window.removeEventListener('keydown', onHotkey);
  }, [showMenu, handlePauseResume, handleQuickSave, handleQuickLoad, handleScreenshot, toggleFullscreen]);

  if (!currentGame) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-lg">No game loaded</div>
      </div>
    );
  }

  const infoParts = [
    frame ? `${frame.width}×${frame.height}` : 'no signal',
    webglStatus,
    audioStatus,
  ].filter(Boolean);

  return (
    <div
      className={`relative h-full bg-black overflow-hidden select-none ${controlsVisible ? '' : 'emu-stage--idle'}`}
      onMouseMove={showControls}
      onMouseDown={showControls}
    >
      {/* Game stage: the canvas is letterboxed inside this full-bleed area
          by layoutCanvas(); its buffer size never participates in layout. */}
      <div
        ref={stageRef}
        className="absolute inset-0"
        onDoubleClick={toggleFullscreen}
      >
        <canvas ref={canvasRef} className="emulator-canvas absolute" />
      </div>

      {/* Transient action feedback */}
      {toast && (
        <div className={`emu-toast ${toast.tone === 'err' ? 'emu-toast--err' : ''}`} role="status">
          {toast.text}
        </div>
      )}

      {/* Paused indicator (hidden while the quick menu is up) */}
      {isPaused && !showMenu && (
        <div className="emu-paused-chip">
          Paused <span className="key-hint">· Space resumes</span>
        </div>
      )}

      <ControlDeck
        visible={controlsVisible}
        gameTitle={currentGame.title}
        isPaused={isPaused}
        speed={speed}
        info={infoParts.join(' · ')}
        isFullscreen={isFullscreen}
        onPauseResume={handlePauseResume}
        onSpeedChange={applySpeed}
        onQuickSave={handleQuickSave}
        onQuickLoad={handleQuickLoad}
        onScreenshot={handleScreenshot}
        onMenu={() => setShowMenu(true)}
        onFullscreen={toggleFullscreen}
        onExit={handleStop}
        onActiveChange={handleDeckActive}
      />

      {/* Menu Overlay */}
      <QuickMenu
        isOpen={showMenu}
        onClose={() => setShowMenu(false)}
        onOpenSettings={onOpenSettings}
        onExitToMenu={handleStop}
        canvasRef={canvasRef}
        gameTitle={currentGame.title}
      />
    </div>
  );
}
