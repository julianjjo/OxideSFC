/**
 * Hooks Module for OxideSFC Frontend
 *
 * Exports all custom React hooks for the application:
 * - useKeyboard: Keyboard input handling
 * - useGamepad: Gamepad input handling
 * - useDatabase: Local database operations
 */

// ============================================================================
// Keyboard Hook
// ============================================================================

export { useKeyboard, DEFAULT_SNES_KEYBOARD_MAPPING } from './useKeyboard';
export type { 
  KeyboardHookConfig, 
  ModifierKeys 
} from './useKeyboard';

// ============================================================================
// Gamepad Hook
// ============================================================================

export { useGamepad, DEFAULT_GAMEPAD_PROFILE } from './useGamepad';
export type { 
  GamepadHookConfig 
} from './useGamepad';

// ============================================================================
// Database Hook
// ============================================================================

export { useDatabase, DatabaseError } from './useDatabase';
export type { 
  DatabaseConfig, 
  DatabaseState 
} from './useDatabase';
