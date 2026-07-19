/**
 * Hooks Module for OxideSFC Frontend
 *
 * Exports all custom React hooks for the application:
 * - useGamepad: Gamepad input handling
 * - useDatabase: Local database operations
 *
 * Keyboard input is handled directly in EmulatorView.tsx /
 * ControllerSettings.tsx against the canonical mapping in
 * domain/keyboardDefaults.ts, not through a hook.
 */

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
