/**
 * useGamepad Hook
 * 
 * React hook for handling gamepad input with support for:
 * - Gamepad API polling
 * - Connection/disconnection handling
 * - Multiple gamepads support
 * - Configurable deadzone for analog sticks
 * - Button and axis mapping to SNES buttons
 * - Event emission via eventBus
 */

import { useEffect, useRef, useCallback, useState } from 'react';
import type { InputButton, GamepadState, GamepadProfile, GamepadButtonMapping, InputState } from '../domain/types';
import { emitGamepadConnect, emitGamepadDisconnect, emitInputButton } from '../services/eventBus';

// ============================================================================
// Configuration Types
// ============================================================================

export interface GamepadHookConfig {
  /** Enable gamepad input handling */
  enabled?: boolean;
  /** Polling interval in ms */
  pollingInterval?: number;
  /** Deadzone threshold for analog sticks (0-1) */
  deadzone?: number;
  /** Custom gamepad profile */
  profile?: GamepadProfile;
  /** Callback when button state changes */
  onButtonChange?: (button: InputButton, pressed: boolean, gamepadIndex: number) => void;
  /** Callback when gamepad connects */
  onConnect?: (gamepad: GamepadState) => void;
  /** Callback when gamepad disconnects */
  onDisconnect?: (index: number) => void;
}

// ============================================================================
// Default Gamepad Profile (Standard Gamepad Layout)
// ============================================================================

export const DEFAULT_GAMEPAD_PROFILE: GamepadProfile = {
  id: 'default',
  name: 'Default',
  is_default: true,
  deadzone: 0.15,
  button_mapping: {
    a: 0,        // A / Cross
    b: 1,        // B / Circle
    x: 2,        // X / Square
    y: 3,        // Y / Triangle
    start: 9,    // Start
    select: 8,   // Select
    l: 4,        // LB
    r: 5,        // RB
    dpad_up: 12,
    dpad_down: 13,
    dpad_left: 14,
    dpad_right: 15,
    l_analog_x: 0,
    l_analog_y: 1,
    r_analog_x: 2,
    r_analog_y: 3,
  },
};

// ============================================================================
// Button Masks (matching SNES controller)
// ============================================================================

const BUTTON_MASK = {
  B: 0x01,
  Y: 0x02,
  SELECT: 0x04,
  START: 0x08,
  UP: 0x10,
  DOWN: 0x20,
  LEFT: 0x40,
  RIGHT: 0x80,
  A: 0x100,
  X: 0x200,
  L: 0x400,
  R: 0x800,
};

// ============================================================================
// Hook Implementation
// ============================================================================

/**
 * useGamepad - Gamepad input hook for SNES emulation
 * 
 * @param config - Configuration options for the hook
 * @returns Object containing gamepad state and utilities
 * 
 * @example
 * ```tsx
 * const { 
 *   connectedGamepads, 
 *   pressedButtons,
 *   getInputState,
 *   setProfile 
 * } = useGamepad({
 *   enabled: true,
 *   deadzone: 0.2,
 *   pollingInterval: 16,
 *   onButtonChange: (button, pressed, index) => console.log(button, pressed, index)
 * });
 * ```
 */
export function useGamepad(config: GamepadHookConfig = {}) {
  const {
    enabled = true,
    pollingInterval = 16,
    deadzone: configDeadzone = 0.15,
    profile: initialProfile,
    onButtonChange,
    onConnect,
    onDisconnect,
  } = config;

  // State
  const [connectedGamepads, setConnectedGamepads] = useState<GamepadState[]>([]);
  const [activeGamepadIndex, setActiveGamepadIndex] = useState<number | null>(null);
  const [pressedButtons, setPressedButtons] = useState<Set<InputButton>>(new Set());

  // Refs
  const profileRef = useRef<GamepadProfile>(initialProfile ?? DEFAULT_GAMEPAD_PROFILE);
  const deadzoneRef = useRef<number>(configDeadzone);
  const pollingIdRef = useRef<number | null>(null);
  const previousButtonStatesRef = useRef<Map<number, Set<InputButton>>>(new Map());
  const gamepadStatesRef = useRef<Map<number, GamepadState>>(new Map());

  // Update profile when prop changes
  useEffect(() => {
    if (initialProfile) {
      profileRef.current = initialProfile;
    }
  }, [initialProfile]);

  // Update deadzone when prop changes
  useEffect(() => {
    deadzoneRef.current = configDeadzone;
  }, [configDeadzone]);

  // Convert browser Gamepad to our GamepadState
  const convertGamepadState = useCallback((gamepad: Gamepad): GamepadState => {
    return {
      index: gamepad.index,
      id: gamepad.id,
      connected: gamepad.connected,
      buttons: Array.from(gamepad.buttons).map((button, index) => ({
        index,
        pressed: button.pressed,
        value: button.value,
      })),
      axes: Array.from(gamepad.axes),
      timestamp: gamepad.timestamp,
    };
  }, []);

  // Apply deadzone to analog value
  const applyDeadzone = useCallback((value: number): number => {
    const dz = deadzoneRef.current;
    if (Math.abs(value) <= dz) {
      return 0;
    }
    // Remap value to exclude deadzone range
    const sign = value > 0 ? 1 : -1;
    return sign * (Math.abs(value) - dz) / (1 - dz);
  }, []);

  // Map gamepad state to input state
  const mapToInputState = useCallback((gamepadState: GamepadState): InputState => {
    const profile = profileRef.current;
    const mapping = profile.button_mapping;
    let buttons = 0;
    let x = 0;
    let y = 0;

    // Process buttons
    gamepadState.buttons.forEach((button, index) => {
      if (button.pressed) {
        // Map gamepad button to SNES button and set bit
        if (index === mapping.a) { buttons |= BUTTON_MASK.A; }
        if (index === mapping.b) { buttons |= BUTTON_MASK.B; }
        if (index === mapping.x) { buttons |= BUTTON_MASK.X; }
        if (index === mapping.y) { buttons |= BUTTON_MASK.Y; }
        if (index === mapping.select) { buttons |= BUTTON_MASK.SELECT; }
        if (index === mapping.start) { buttons |= BUTTON_MASK.START; }
        if (index === mapping.l) { buttons |= BUTTON_MASK.L; }
        if (index === mapping.r) { buttons |= BUTTON_MASK.R; }
        if (index === mapping.dpad_up) { buttons |= BUTTON_MASK.UP; y = -1; }
        if (index === mapping.dpad_down) { buttons |= BUTTON_MASK.DOWN; y = 1; }
        if (index === mapping.dpad_left) { buttons |= BUTTON_MASK.LEFT; x = -1; }
        if (index === mapping.dpad_right) { buttons |= BUTTON_MASK.RIGHT; x = 1; }
      }
    });

    // Process analog sticks (left stick for D-pad emulation)
    if (gamepadState.axes.length > mapping.l_analog_x && gamepadState.axes.length > mapping.l_analog_y) {
      const lx = applyDeadzone(gamepadState.axes[mapping.l_analog_x]);
      const ly = applyDeadzone(gamepadState.axes[mapping.l_analog_y]);

      // Use analog input if D-pad not pressed
      if (x === 0 && y === 0) {
        if (ly < -0.5) { buttons |= BUTTON_MASK.UP; y = -1; }
        else if (ly > 0.5) { buttons |= BUTTON_MASK.DOWN; y = 1; }
        
        if (lx < -0.5) { buttons |= BUTTON_MASK.LEFT; x = -1; }
        else if (lx > 0.5) { buttons |= BUTTON_MASK.RIGHT; x = 1; }
      }
    }

    return { buttons, x, y };
  }, [applyDeadzone]);

  // Convert an InputState's button bitmask into the set of pressed InputButtons
  const inputStateToButtonSet = useCallback((inputState: InputState): Set<InputButton> => {
    const buttons = new Set<InputButton>();
    if (inputState.buttons & BUTTON_MASK.A) buttons.add('a');
    if (inputState.buttons & BUTTON_MASK.B) buttons.add('b');
    if (inputState.buttons & BUTTON_MASK.X) buttons.add('x');
    if (inputState.buttons & BUTTON_MASK.Y) buttons.add('y');
    if (inputState.buttons & BUTTON_MASK.SELECT) buttons.add('select');
    if (inputState.buttons & BUTTON_MASK.START) buttons.add('start');
    if (inputState.buttons & BUTTON_MASK.L) buttons.add('l');
    if (inputState.buttons & BUTTON_MASK.R) buttons.add('r');
    if (inputState.buttons & BUTTON_MASK.UP) buttons.add('up');
    if (inputState.buttons & BUTTON_MASK.DOWN) buttons.add('down');
    if (inputState.buttons & BUTTON_MASK.LEFT) buttons.add('left');
    if (inputState.buttons & BUTTON_MASK.RIGHT) buttons.add('right');
    return buttons;
  }, []);

  // Handle button state changes
  const handleButtonChanges = useCallback((gamepadIndex: number, newState: GamepadState) => {
    const previousState = previousButtonStatesRef.current.get(gamepadIndex) ?? new Set<InputButton>();
    const currentButtons = new Set<InputButton>();
    const profile = profileRef.current;
    const mapping = profile.button_mapping;

    // Determine current button states
    newState.buttons.forEach((button, index) => {
      if (button.pressed) {
        if (index === mapping.a) currentButtons.add('a');
        if (index === mapping.b) currentButtons.add('b');
        if (index === mapping.x) currentButtons.add('x');
        if (index === mapping.y) currentButtons.add('y');
        if (index === mapping.select) currentButtons.add('select');
        if (index === mapping.start) currentButtons.add('start');
        if (index === mapping.l) currentButtons.add('l');
        if (index === mapping.r) currentButtons.add('r');
        if (index === mapping.dpad_up) currentButtons.add('up');
        if (index === mapping.dpad_down) currentButtons.add('down');
        if (index === mapping.dpad_left) currentButtons.add('left');
        if (index === mapping.dpad_right) currentButtons.add('right');
      }
    });

    // Check for button press events
    currentButtons.forEach(button => {
      if (!previousState.has(button)) {
        emitInputButton(button, true);
        onButtonChange?.(button, true, gamepadIndex);
      }
    });

    // Check for button release events
    previousState.forEach(button => {
      if (!currentButtons.has(button)) {
        emitInputButton(button, false);
        onButtonChange?.(button, false, gamepadIndex);
      }
    });

    // Update previous state
    previousButtonStatesRef.current.set(gamepadIndex, new Set(currentButtons));

    return currentButtons;
  }, [onButtonChange]);

  // Poll gamepads
  const pollGamepads = useCallback(() => {
    if (!enabled) return;

    const gamepads = navigator.getGamepads();
    const newStates: GamepadState[] = [];
    let newActiveIndex = activeGamepadIndex;

    for (const gamepad of gamepads) {
      if (gamepad) {
        const state = convertGamepadState(gamepad);
        gamepadStatesRef.current.set(gamepad.index, state);
        newStates.push(state);

        // Auto-select first gamepad if none selected
        if (newActiveIndex === null) {
          newActiveIndex = gamepad.index;
        }

        // Handle button changes
        handleButtonChanges(gamepad.index, state);
      }
    }

    // Update all states at once
    setConnectedGamepads(newStates);
    
    if (newActiveIndex !== activeGamepadIndex) {
      setActiveGamepadIndex(newActiveIndex);
    }

    // Calculate combined pressed buttons for active gamepad
    if (newActiveIndex !== null) {
      const activeState = gamepadStatesRef.current.get(newActiveIndex);
      if (activeState) {
        setPressedButtons(inputStateToButtonSet(mapToInputState(activeState)));
      }
    }
  }, [enabled, activeGamepadIndex, convertGamepadState, handleButtonChanges, mapToInputState, inputStateToButtonSet]);

  // Handle gamepad connected
  const handleGamepadConnected = useCallback((event: GamepadEvent) => {
    const gamepad = event.gamepad;
    const state = convertGamepadState(gamepad);

    gamepadStatesRef.current.set(gamepad.index, state);
    emitGamepadConnect(state);
    onConnect?.(state);

    // Start polling if not already
    if (enabled && pollingIdRef.current === null) {
      pollingIdRef.current = window.setInterval(pollGamepads, pollingInterval);
    }
  }, [enabled, pollingInterval, pollGamepads, convertGamepadState, onConnect]);

  // Handle gamepad disconnected
  const handleGamepadDisconnected = useCallback((event: GamepadEvent) => {
    const index = event.gamepad.index;
    
    gamepadStatesRef.current.delete(index);
    previousButtonStatesRef.current.delete(index);
    emitGamepadDisconnect(index);
    onDisconnect?.(index);

    // Clear active gamepad if it was disconnected
    if (activeGamepadIndex === index) {
      setActiveGamepadIndex(null);
      setPressedButtons(new Set());
    }

    // Stop polling if no gamepads
    if (gamepadStatesRef.current.size === 0 && pollingIdRef.current !== null) {
      clearInterval(pollingIdRef.current);
      pollingIdRef.current = null;
    }
  }, [activeGamepadIndex, onDisconnect]);

  // Set up event listeners and polling
  useEffect(() => {
    if (!enabled) return;

    // Check for already connected gamepads
    const gamepads = navigator.getGamepads();
    for (const gamepad of gamepads) {
      if (gamepad) {
        const state = convertGamepadState(gamepad);
        gamepadStatesRef.current.set(gamepad.index, state);
      }
    }

    // Add event listeners
    window.addEventListener('gamepadconnected', handleGamepadConnected);
    window.addEventListener('gamepaddisconnected', handleGamepadDisconnected);

    // Start polling if gamepads are connected
    if (gamepadStatesRef.current.size > 0) {
      pollingIdRef.current = window.setInterval(pollGamepads, pollingInterval);
    }

    return () => {
      window.removeEventListener('gamepadconnected', handleGamepadConnected);
      window.removeEventListener('gamepaddisconnected', handleGamepadDisconnected);

      if (pollingIdRef.current !== null) {
        clearInterval(pollingIdRef.current);
        pollingIdRef.current = null;
      }
    };
  }, [enabled, pollingInterval, pollGamepads, convertGamepadState, handleGamepadConnected, handleGamepadDisconnected]);

  // Get input state for a specific port
  const getInputState = useCallback((port: number = 0): InputState => {
    const gamepadState = gamepadStatesRef.current.get(port);
    
    if (!gamepadState) {
      return { buttons: 0, x: 0, y: 0 };
    }

    return mapToInputState(gamepadState);
  }, [mapToInputState]);

  // Set active gamepad
  const setActiveGamepad = useCallback((index: number | null) => {
    setActiveGamepadIndex(index);
    
    if (index !== null) {
      const state = gamepadStatesRef.current.get(index);
      if (state) {
        // Update pressed buttons for new active gamepad
        setPressedButtons(inputStateToButtonSet(mapToInputState(state)));
        handleButtonChanges(index, state);
      }
    } else {
      setPressedButtons(new Set());
    }
  }, [mapToInputState, handleButtonChanges, inputStateToButtonSet]);

  // Set gamepad profile
  const setProfile = useCallback((profile: GamepadProfile) => {
    profileRef.current = profile;
  }, []);

  // Set deadzone
  const setDeadzone = useCallback((value: number) => {
    deadzoneRef.current = Math.max(0, Math.min(1, value));
  }, []);

  // Check if a specific button is pressed
  const isButtonPressed = useCallback((button: InputButton): boolean => {
    return pressedButtons.has(button);
  }, [pressedButtons]);

  // Get all pressed buttons as array
  const getPressedButtons = useCallback((): InputButton[] => {
    return Array.from(pressedButtons);
  }, [pressedButtons]);

  // Get gamepad by index
  const getGamepad = useCallback((index: number): GamepadState | undefined => {
    return gamepadStatesRef.current.get(index);
  }, []);

  return {
    // State
    connectedGamepads,
    activeGamepadIndex,
    pressedButtons,
    
    // Methods
    getInputState,
    setActiveGamepad,
    setProfile,
    setDeadzone,
    isButtonPressed,
    getPressedButtons,
    getGamepad,
  };
}

// ============================================================================
// Type Exports
// ============================================================================

export type { GamepadProfile, GamepadButtonMapping };
