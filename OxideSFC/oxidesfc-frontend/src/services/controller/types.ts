/**
 * Controller Profile Types
 * 
 * Type definitions for the controller profile system.
 */



// ============================================================================
// Profile Type
// ============================================================================

export type ControllerProfileType = 'keyboard' | 'gamepad';

// ============================================================================
// Analog Stick Configuration
// ============================================================================

export interface AnalogStickConfig {
  /** Whether the axis is inverted */
  invertX: boolean;
  invertY: boolean;
  /** Deadzone threshold (0-1) */
  deadzone: number;
  /** Sensitivity multiplier (0.5-2.0) */
  sensitivity: number;
}

// ============================================================================
// Controller Profile
// ============================================================================

export interface ControllerProfile {
  /** Unique identifier */
  id: string;
  /** Profile name */
  name: string;
  /** Profile type */
  type: ControllerProfileType;
  /** Gamepad index (for gamepad profiles) */
  gamepadIndex?: number;
  /** Button mappings */
  buttonMapping: ButtonMapping;
  /** Analog stick configuration */
  analogConfig: AnalogStickConfig;
  /** Whether this is the default profile */
  isDefault: boolean;
  /** Game ID for per-game defaults (null for global) */
  gameId?: string;
  /** Timestamps */
  createdAt: string;
  updatedAt: string;
}

// ============================================================================
// Button Mapping (keyboard or gamepad)
// ============================================================================

export interface ButtonMapping {
  // Directional
  up: string;
  down: string;
  left: string;
  right: string;
  // Face buttons
  a: string;
  b: string;
  x: string;
  y: string;
  // Shoulder buttons
  l: string;
  r: string;
  // System buttons
  start: string;
  select: string;
}

// ============================================================================
// Available Input Sources
// ============================================================================

export interface InputSource {
  /** The key code or button index */
  id: string;
  /** Display name */
  label: string;
  /** Type */
  type: ControllerProfileType;
}

// ============================================================================
// Default Keyboard Mappings
// ============================================================================

export const DEFAULT_KEYBOARD_MAPPING: ButtonMapping = {
  up: 'ArrowUp',
  down: 'ArrowDown',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  a: 'KeyZ',
  b: 'KeyX',
  x: 'KeyA',
  y: 'KeyS',
  l: 'KeyQ',
  r: 'KeyW',
  start: 'Enter',
  select: 'ShiftRight',
};

// ============================================================================
// Default Gamepad Mappings (Xbox-style)
// ============================================================================

export const DEFAULT_GAMEPAD_MAPPING: ButtonMapping = {
  up: '12',    // D-Pad Up
  down: '13',  // D-Pad Down
  left: '14',  // D-Pad Left
  right: '15', // D-Pad Right
  a: '0',      // A / Cross
  b: '1',      // B / Circle
  x: '2',      // X / Square
  y: '3',      // Y / Triangle
  l: '4',       // L1
  r: '5',       // R1
  start: '9',   // Start
  select: '8',  // Select
};

// ============================================================================
// Default Analog Configuration
// ============================================================================

export const DEFAULT_ANALOG_CONFIG: AnalogStickConfig = {
  invertX: false,
  invertY: false,
  deadzone: 0.15,
  sensitivity: 1.0,
};

// ============================================================================
// Preset Profiles
// ============================================================================

export interface ProfilePreset {
  id: string;
  name: string;
  type: ControllerProfileType;
  mapping: ButtonMapping;
  analogConfig?: AnalogStickConfig;
}

export const KEYBOARD_PRESETS: ProfilePreset[] = [
  {
    id: 'keyboard-default',
    name: 'Default',
    type: 'keyboard',
    mapping: DEFAULT_KEYBOARD_MAPPING,
  },
  {
    id: 'keyboard-wasd',
    name: 'WASD',
    type: 'keyboard',
    mapping: {
      up: 'KeyW',
      down: 'KeyS',
      left: 'KeyA',
      right: 'KeyD',
      a: 'KeyK',
      b: 'KeyJ',
      x: 'KeyI',
      y: 'KeyU',
      l: 'KeyO',
      r: 'KeyP',
      start: 'Enter',
      select: 'ShiftRight',
    },
  },
  {
    id: 'keyboard-vim',
    name: 'VIM-style',
    type: 'keyboard',
    mapping: {
      up: 'KeyK',
      down: 'KeyJ',
      left: 'KeyH',
      right: 'KeyL',
      a: 'KeyN',
      b: 'KeyM',
      x: 'KeyB',
      y: 'KeyV',
      l: 'KeyG',
      r: 'KeyT',
      start: 'Enter',
      select: 'ShiftRight',
    },
  },
];

export const GAMEPAD_PRESETS: ProfilePreset[] = [
  {
    id: 'gamepad-xbox',
    name: 'Xbox Controller',
    type: 'gamepad',
    mapping: DEFAULT_GAMEPAD_MAPPING,
  },
  {
    id: 'gamepad-playstation',
    name: 'PlayStation Controller',
    type: 'gamepad',
    mapping: {
      up: '12',
      down: '13',
      left: '14',
      right: '15',
      a: '1',    // Circle
      b: '0',     // Cross
      x: '2',    // Square
      y: '3',    // Triangle
      l: '4',
      r: '5',
      start: '9',
      select: '8',
    },
  },
  {
    id: 'gamepad-switch',
    name: 'Nintendo Switch Controller',
    type: 'gamepad',
    mapping: {
      up: '12',
      down: '13',
      left: '14',
      right: '15',
      a: '0',    // B / A (Switch layout)
      b: '1',    // A / B
      x: '2',    // Y / X
      y: '3',    // X / Y
      l: '4',
      r: '5',
      start: '9',
      select: '8',
    },
  },
  {
    id: 'gamepad-generic',
    name: 'Generic Gamepad',
    type: 'gamepad',
    mapping: {
      up: '12',
      down: '13',
      left: '14',
      right: '15',
      a: '0',
      b: '1',
      x: '2',
      y: '3',
      l: '4',
      r: '5',
      start: '9',
      select: '8',
    },
    analogConfig: {
      ...DEFAULT_ANALOG_CONFIG,
      deadzone: 0.2,
    },
  },
];
