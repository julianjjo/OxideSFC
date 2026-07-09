/**
 * Input Manager
 * 
 * Manages keyboard and gamepad input, combining them into a unified
 * input system that can send input states to the emulation backend.
 */

import type { InputButton, KeyboardMapping, GamepadProfile, GamepadState, InputState } from '../../domain/types';
import { emitInputButton, emitGamepadConnect, emitGamepadDisconnect } from '../../services/eventBus';

// ============================================================================
// Input Manager Types
// ============================================================================

/**
 * Input manager configuration
 */
export interface InputManagerConfig {
  /**
   * Enable keyboard input
   */
  keyboardEnabled: boolean;
  
  /**
   * Enable gamepad input
   */
  gamepadEnabled: boolean;
  
  /**
   * Polling interval for gamepad in ms
   */
  gamepadPollingInterval: number;
  
  /**
   * Deadzone for analog sticks
   */
  analogDeadzone: number;
}

/**
 * Default input manager configuration
 */
export const DEFAULT_INPUT_CONFIG: InputManagerConfig = {
  keyboardEnabled: true,
  gamepadEnabled: true,
  gamepadPollingInterval: 16, // ~60fps
  analogDeadzone: 0.15,
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
// Input Manager Implementation
// ============================================================================

/**
 * Input Manager
 * 
 * Manages keyboard and gamepad input, combining them into a unified
 * input system for the emulation backend.
 */
class InputManagerImpl {
  private config: InputManagerConfig;
  private keyboardMapping: KeyboardMapping = {};
  private gamepadProfile: GamepadProfile | null = null;
  private pressedButtons: Set<InputButton> = new Set();
  private gamepads: Map<number, GamepadState> = new Map();
  private gamepadPollingId: number | null = null;
  private inputCallback: ((port: number, input: InputState) => void) | null = null;
  private eventHandlers: Map<string, Set<(data: unknown) => void>> = new Map();

  // Bound event handlers, created once so destroy() can remove the exact
  // same function references that addEventListener registered. Calling
  // `.bind(this)` again at removeEventListener time would create a brand
  // new function object that never matches, silently leaving the original
  // listeners attached (a leak across repeated mount/unmount).
  private boundHandleKeyDown = this.handleKeyDown.bind(this);
  private boundHandleKeyUp = this.handleKeyUp.bind(this);
  private boundHandleGamepadConnected = this.handleGamepadConnected.bind(this);
  private boundHandleGamepadDisconnected = this.handleGamepadDisconnected.bind(this);

  constructor(config: Partial<InputManagerConfig> = {}) {
    this.config = { ...DEFAULT_INPUT_CONFIG, ...config };

    // Set up event listeners
    this.setupKeyboardListeners();
    this.setupGamepadListeners();
  }

  // ==========================================================================
  // Configuration Methods
  // ==========================================================================

  /**
   * Set keyboard mapping
   */
  setKeyboardMapping(mapping: KeyboardMapping): void {
    this.keyboardMapping = mapping;
  }

  /**
   * Set gamepad profile
   */
  setGamepadProfile(profile: GamepadProfile | null): void {
    this.gamepadProfile = profile;
  }

  /**
   * Set input callback
   */
  setInputCallback(callback: (port: number, input: InputState) => void): void {
    this.inputCallback = callback;
  }

  /**
   * Enable/disable keyboard
   */
  setKeyboardEnabled(enabled: boolean): void {
    this.config.keyboardEnabled = enabled;
  }

  /**
   * Enable/disable gamepad
   */
  setGamepadEnabled(enabled: boolean): void {
    this.config.gamepadEnabled = enabled;
    
    if (!enabled) {
      this.stopGamepadPolling();
    } else {
      this.startGamepadPolling();
    }
  }

  // ==========================================================================
  // Input State Methods
  // ==========================================================================

  /**
   * Get currently pressed buttons
   */
  getPressedButtons(): InputButton[] {
    return Array.from(this.pressedButtons);
  }

  /**
   * Check if a button is pressed
   */
  isButtonPressed(button: InputButton): boolean {
    return this.pressedButtons.has(button);
  }

  /**
   * Get connected gamepads
   */
  getGamepads(): GamepadState[] {
    return Array.from(this.gamepads.values());
  }

  /**
   * Get input state for a port
   */
  getInputState(port: number): InputState {
    let buttons = 0;
    let x = 0;
    let y = 0;

    // Process keyboard input
    if (this.config.keyboardEnabled) {
      const keyboardState = this.getKeyboardInputState();
      buttons |= keyboardState.buttons;
      x = keyboardState.x;
      y = keyboardState.y;
    }

    // Process gamepad input
    if (this.config.gamepadEnabled) {
      const gamepadState = this.getGamepadInputState(port);
      buttons |= gamepadState.buttons;
      
      // Combine analog input (keyboard takes precedence)
      if (x === 0 && y === 0) {
        x = gamepadState.x;
        y = gamepadState.y;
      }
    }

    return { buttons, x, y };
  }

  /**
   * Send input to backend
   */
  sendInput(port: number): void {
    const input = this.getInputState(port);
    
    if (this.inputCallback) {
      this.inputCallback(port, input);
    }
  }

  // ==========================================================================
  // Keyboard Input
  // ==========================================================================

  /**
   * Get keyboard input state
   */
  private getKeyboardInputState(): InputState {
    let buttons = 0;
    let x = 0;
    let y = 0;

    // Note: keyboard press/release state is tracked directly by
    // handleKeyDown/handleKeyUp mutating pressedButtons; this method just
    // reads that state below.

    // Build button mask
    if (this.pressedButtons.has('a')) buttons |= BUTTON_MASK.A;
    if (this.pressedButtons.has('b')) buttons |= BUTTON_MASK.B;
    if (this.pressedButtons.has('x')) buttons |= BUTTON_MASK.X;
    if (this.pressedButtons.has('y')) buttons |= BUTTON_MASK.Y;
    if (this.pressedButtons.has('select')) buttons |= BUTTON_MASK.SELECT;
    if (this.pressedButtons.has('start')) buttons |= BUTTON_MASK.START;
    if (this.pressedButtons.has('l')) buttons |= BUTTON_MASK.L;
    if (this.pressedButtons.has('r')) buttons |= BUTTON_MASK.R;

    // D-pad
    if (this.pressedButtons.has('up')) {
      buttons |= BUTTON_MASK.UP;
      y = -1;
    }
    if (this.pressedButtons.has('down')) {
      buttons |= BUTTON_MASK.DOWN;
      y = 1;
    }
    if (this.pressedButtons.has('left')) {
      buttons |= BUTTON_MASK.LEFT;
      x = -1;
    }
    if (this.pressedButtons.has('right')) {
      buttons |= BUTTON_MASK.RIGHT;
      x = 1;
    }

    return { buttons, x, y };
  }

  // ==========================================================================
  // Gamepad Input
  // ==========================================================================

  /**
   * Get gamepad input state for a port
   */
  private getGamepadInputState(port: number): InputState {
    const gamepad = this.gamepads.get(port);
    
    if (!gamepad || !this.gamepadProfile) {
      return { buttons: 0, x: 0, y: 0 };
    }

    let buttons = 0;
    let x = 0;
    let y = 0;

    const mapping = this.gamepadProfile.button_mapping;

    // Process buttons
    gamepad.buttons.forEach((button, index) => {
      // Map gamepad button to SNES button, reporting both press AND release
      // so a button that was pressed and then released doesn't stay "held"
      // forever in the shared pressedButtons set.
      if (index === mapping.a) this.setButton('a', button.pressed);
      if (index === mapping.b) this.setButton('b', button.pressed);
      if (index === mapping.x) this.setButton('x', button.pressed);
      if (index === mapping.y) this.setButton('y', button.pressed);
      if (index === mapping.start) this.setButton('start', button.pressed);
      if (index === mapping.select) this.setButton('select', button.pressed);
      if (index === mapping.l) this.setButton('l', button.pressed);
      if (index === mapping.r) this.setButton('r', button.pressed);
      if (index === mapping.dpad_up) this.setButton('up', button.pressed);
      if (index === mapping.dpad_down) this.setButton('down', button.pressed);
      if (index === mapping.dpad_left) this.setButton('left', button.pressed);
      if (index === mapping.dpad_right) this.setButton('right', button.pressed);
    });

    // Process analog sticks
    if (gamepad.axes.length > 0) {
      const lx = gamepad.axes[mapping.l_analog_x] ?? 0;
      const ly = gamepad.axes[mapping.l_analog_y] ?? 0;
      
      // Apply deadzone
      const deadzone = this.config.analogDeadzone;
      if (Math.abs(lx) > deadzone) x = lx;
      if (Math.abs(ly) > deadzone) y = ly;
    }

    // Build button mask
    if (this.pressedButtons.has('a')) buttons |= BUTTON_MASK.A;
    if (this.pressedButtons.has('b')) buttons |= BUTTON_MASK.B;
    if (this.pressedButtons.has('x')) buttons |= BUTTON_MASK.X;
    if (this.pressedButtons.has('y')) buttons |= BUTTON_MASK.Y;
    if (this.pressedButtons.has('select')) buttons |= BUTTON_MASK.SELECT;
    if (this.pressedButtons.has('start')) buttons |= BUTTON_MASK.START;
    if (this.pressedButtons.has('l')) buttons |= BUTTON_MASK.L;
    if (this.pressedButtons.has('r')) buttons |= BUTTON_MASK.R;
    if (this.pressedButtons.has('up')) { buttons |= BUTTON_MASK.UP; y = -1; }
    if (this.pressedButtons.has('down')) { buttons |= BUTTON_MASK.DOWN; y = 1; }
    if (this.pressedButtons.has('left')) { buttons |= BUTTON_MASK.LEFT; x = -1; }
    if (this.pressedButtons.has('right')) { buttons |= BUTTON_MASK.RIGHT; x = 1; }

    return { buttons, x, y };
  }

  // ==========================================================================
  // Button State Management
  // ==========================================================================

  /**
   * Set button state
   */
  private setButton(button: InputButton, pressed: boolean): void {
    const wasPressed = this.pressedButtons.has(button);
    
    if (pressed && !wasPressed) {
      this.pressedButtons.add(button);
      emitInputButton(button, true);
      this.emit('button', { button, pressed: true });
    } else if (!pressed && wasPressed) {
      this.pressedButtons.delete(button);
      emitInputButton(button, false);
      this.emit('button', { button, pressed: false });
    }
  }

  // ==========================================================================
  // Event Listeners
  // ==========================================================================

  /**
   * Set up keyboard event listeners
   */
  private setupKeyboardListeners(): void {
    window.addEventListener('keydown', this.boundHandleKeyDown);
    window.addEventListener('keyup', this.boundHandleKeyUp);
  }

  /**
   * Handle key down
   */
  private handleKeyDown(event: KeyboardEvent): void {
    if (!this.config.keyboardEnabled) return;
    
    const button = this.keyboardMapping[event.code];
    if (button) {
      this.setButton(button, true);
      event.preventDefault();
    }
  }

  /**
   * Handle key up
   */
  private handleKeyUp(event: KeyboardEvent): void {
    if (!this.config.keyboardEnabled) return;
    
    const button = this.keyboardMapping[event.code];
    if (button) {
      this.setButton(button, false);
      event.preventDefault();
    }
  }

  /**
   * Set up gamepad event listeners
   */
  private setupGamepadListeners(): void {
    window.addEventListener('gamepadconnected', this.boundHandleGamepadConnected);
    window.addEventListener('gamepaddisconnected', this.boundHandleGamepadDisconnected);
  }

  /**
   * Handle gamepad connected
   */
  private handleGamepadConnected(event: GamepadEvent): void {
    const gamepad = event.gamepad;
    const state = this.convertGamepadState(gamepad);
    
    this.gamepads.set(gamepad.index, state);
    emitGamepadConnect(state);
    this.emit('gamepad:connect', state);

    // Start polling if not already
    if (this.config.gamepadEnabled) {
      this.startGamepadPolling();
    }
  }

  /**
   * Handle gamepad disconnected
   */
  private handleGamepadDisconnected(event: GamepadEvent): void {
    this.gamepads.delete(event.gamepad.index);
    emitGamepadDisconnect(event.gamepad.index);
    this.emit('gamepad:disconnect', event.gamepad.index);

    // Stop polling if no gamepads
    if (this.gamepads.size === 0) {
      this.stopGamepadPolling();
    }
  }

  /**
   * Convert browser Gamepad to our GamepadState
   */
  private convertGamepadState(gamepad: Gamepad): GamepadState {
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
  }

  // ==========================================================================
  // Gamepad Polling
  // ==========================================================================

  /**
   * Start gamepad polling
   */
  private startGamepadPolling(): void {
    if (this.gamepadPollingId !== null) return;
    
    this.gamepadPollingId = window.setInterval(() => {
      this.pollGamepads();
    }, this.config.gamepadPollingInterval);
  }

  /**
   * Stop gamepad polling
   */
  private stopGamepadPolling(): void {
    if (this.gamepadPollingId !== null) {
      clearInterval(this.gamepadPollingId);
      this.gamepadPollingId = null;
    }
  }

  /**
   * Poll gamepads for state changes
   */
  private pollGamepads(): void {
    const gamepads = navigator.getGamepads();
    
    for (const gamepad of gamepads) {
      if (gamepad) {
        const state = this.convertGamepadState(gamepad);
        this.gamepads.set(gamepad.index, state);
      }
    }
  }

  // ==========================================================================
  // Event System
  // ==========================================================================

  /**
   * Subscribe to input manager events
   */
  on(event: string, handler: (data: unknown) => void): () => void {
    if (!this.eventHandlers.has(event)) {
      this.eventHandlers.set(event, new Set());
    }
    
    this.eventHandlers.get(event)!.add(handler);
    
    return () => {
      this.eventHandlers.get(event)?.delete(handler);
    };
  }

  /**
   * Emit an event
   */
  private emit(event: string, data: unknown): void {
    const handlers = this.eventHandlers.get(event);
    if (handlers) {
      handlers.forEach(handler => {
        try {
          handler(data);
        } catch (error) {
          console.error(`Error in InputManager event handler for ${event}:`, error);
        }
      });
    }
  }

  // ==========================================================================
  // Cleanup
  // ==========================================================================

  /**
   * Destroy the input manager
   */
  destroy(): void {
    window.removeEventListener('keydown', this.boundHandleKeyDown);
    window.removeEventListener('keyup', this.boundHandleKeyUp);
    window.removeEventListener('gamepadconnected', this.boundHandleGamepadConnected);
    window.removeEventListener('gamepaddisconnected', this.boundHandleGamepadDisconnected);

    this.stopGamepadPolling();
    this.pressedButtons.clear();
    this.gamepads.clear();
  }
}

// ============================================================================
// Singleton Instance
// ============================================================================

let inputManagerInstance: InputManagerImpl | null = null;

/**
 * Get the input manager singleton
 */
export function getInputManager(): InputManagerImpl {
  if (!inputManagerInstance) {
    inputManagerInstance = new InputManagerImpl();
  }
  return inputManagerInstance;
}


