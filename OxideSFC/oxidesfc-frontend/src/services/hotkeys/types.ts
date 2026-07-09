/**
 * Hotkey Types
 * 
 * Type definitions for the hotkey system.
 */

import type { InputButton } from '../../domain/types';

// ============================================================================
// Hotkey Category
// ============================================================================

export type HotkeyCategory = 'general' | 'emulation' | 'library' | 'navigation';

// ============================================================================
// Hotkey Scope
// ============================================================================

export type HotkeyScope = 'global' | 'local';

// ============================================================================
// Modifier Keys
// ============================================================================

export interface HotkeyModifiers {
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
  meta?: boolean;
}

// ============================================================================
// Hotkey Binding
// ============================================================================

export interface HotkeyBinding {
  /** Unique identifier for this binding */
  id: string;
  /** Human-readable name */
  name: string;
  /** Description of what this hotkey does */
  description: string;
  /** The action identifier */
  action: string;
  /** Key code (e.g., 'F1', 'KeyA', 'ArrowUp') */
  key: string;
  /** Modifier keys required */
  modifiers: HotkeyModifiers;
  /** Category for organization */
  category: HotkeyCategory;
  /** Scope: global or local */
  scope: HotkeyScope;
  /** Contexts where this hotkey is active (for local hotkeys) */
  contexts?: string[];
  /** Whether this hotkey is enabled */
  enabled: boolean;
  /** Default binding (for reset) */
  defaultKey?: string;
  /** Default modifiers */
  defaultModifiers?: HotkeyModifiers;
}

// ============================================================================
// Hotkey Event
// ============================================================================

export interface HotkeyEvent {
  /** The action that was triggered */
  action: string;
  /** The key that was pressed */
  key: string;
  /** The modifiers that were active */
  modifiers: HotkeyModifiers;
  /** Timestamp of the event */
  timestamp: number;
}

// ============================================================================
// Hotkey Conflict
// ============================================================================

export interface HotkeyConflict {
  /** First binding in conflict */
  binding1: HotkeyBinding;
  /** Second binding in conflict */
  binding2: HotkeyBinding;
  /** Description of the conflict */
  description: string;
}

// ============================================================================
// Default Hotkey Mappings
// ============================================================================

export const DEFAULT_HOTKEYS: HotkeyBinding[] = [
  // General
  {
    id: 'hotkey-f1',
    name: 'Help',
    description: 'Show help overlay with all hotkeys',
    action: 'help',
    key: 'F1',
    modifiers: {},
    category: 'general',
    scope: 'global',
    enabled: true,
    defaultKey: 'F1',
    defaultModifiers: {},
  },
  {
    id: 'hotkey-ctrl-s',
    name: 'Settings',
    description: 'Open settings',
    action: 'settings',
    key: 'KeyS',
    modifiers: { ctrl: true },
    category: 'general',
    scope: 'global',
    enabled: true,
    defaultKey: 'KeyS',
    defaultModifiers: { ctrl: true },
  },
  
  // Emulation
  {
    id: 'hotkey-f5',
    name: 'Quick Save',
    description: 'Create a quick save',
    action: 'quicksave',
    key: 'F5',
    modifiers: {},
    category: 'emulation',
    scope: 'local',
    contexts: ['emulation'],
    enabled: true,
    defaultKey: 'F5',
    defaultModifiers: {},
  },
  {
    id: 'hotkey-f9',
    name: 'Quick Load',
    description: 'Load the most recent quick save',
    action: 'quickload',
    key: 'F9',
    modifiers: {},
    category: 'emulation',
    scope: 'local',
    contexts: ['emulation'],
    enabled: true,
    defaultKey: 'F9',
    defaultModifiers: {},
  },
  {
    id: 'hotkey-f8',
    name: 'Screenshot',
    description: 'Take a screenshot',
    action: 'screenshot',
    key: 'F8',
    modifiers: {},
    category: 'emulation',
    scope: 'local',
    contexts: ['emulation'],
    enabled: true,
    defaultKey: 'F8',
    defaultModifiers: {},
  },
  {
    id: 'hotkey-p',
    name: 'Pause/Resume',
    description: 'Pause or resume emulation',
    action: 'pause',
    key: 'KeyP',
    modifiers: {},
    category: 'emulation',
    scope: 'local',
    contexts: ['emulation'],
    enabled: true,
    defaultKey: 'KeyP',
    defaultModifiers: {},
  },
  {
    id: 'hotkey-escape',
    name: 'Exit to Menu',
    description: 'Exit emulation and return to menu',
    action: 'exit',
    key: 'Escape',
    modifiers: {},
    category: 'emulation',
    scope: 'local',
    contexts: ['emulation'],
    enabled: true,
    defaultKey: 'Escape',
    defaultModifiers: {},
  },
  {
    id: 'hotkey-ctrl-r',
    name: 'Reset',
    description: 'Reset the current game',
    action: 'reset',
    key: 'KeyR',
    modifiers: { ctrl: true },
    category: 'emulation',
    scope: 'local',
    contexts: ['emulation'],
    enabled: true,
    defaultKey: 'KeyR',
    defaultModifiers: { ctrl: true },
  },
  
  // Navigation
  {
    id: 'hotkey-ctrl-f',
    name: 'Search',
    description: 'Focus search input',
    action: 'search',
    key: 'KeyF',
    modifiers: { ctrl: true },
    category: 'navigation',
    scope: 'global',
    enabled: true,
    defaultKey: 'KeyF',
    defaultModifiers: { ctrl: true },
  },
  {
    id: 'hotkey-ctrl-1',
    name: 'Tab 1',
    description: 'Navigate to first tab',
    action: 'tab1',
    key: 'Digit1',
    modifiers: { ctrl: true },
    category: 'navigation',
    scope: 'global',
    enabled: true,
    defaultKey: 'Digit1',
    defaultModifiers: { ctrl: true },
  },
  {
    id: 'hotkey-ctrl-2',
    name: 'Tab 2',
    description: 'Navigate to second tab',
    action: 'tab2',
    key: 'Digit2',
    modifiers: { ctrl: true },
    category: 'navigation',
    scope: 'global',
    enabled: true,
    defaultKey: 'Digit2',
    defaultModifiers: { ctrl: true },
  },
  {
    id: 'hotkey-ctrl-3',
    name: 'Tab 3',
    description: 'Navigate to third tab',
    action: 'tab3',
    key: 'Digit3',
    modifiers: { ctrl: true },
    category: 'navigation',
    scope: 'global',
    enabled: true,
    defaultKey: 'Digit3',
    defaultModifiers: { ctrl: true },
  },
  
  // Library
  {
    id: 'hotkey-f2',
    name: 'Refresh Library',
    description: 'Scan folders for new games',
    action: 'refresh',
    key: 'F2',
    modifiers: {},
    category: 'library',
    scope: 'global',
    enabled: true,
    defaultKey: 'F2',
    defaultModifiers: {},
  },
  {
    id: 'hotkey-f3',
    name: 'Add to Favorites',
    description: 'Toggle favorite status for selected game',
    action: 'favorite',
    key: 'F3',
    modifiers: {},
    category: 'library',
    scope: 'local',
    contexts: ['library'],
    enabled: true,
    defaultKey: 'F3',
    defaultModifiers: {},
  },
  {
    id: 'hotkey-delete',
    name: 'Delete Game',
    description: 'Remove game from library',
    action: 'delete',
    key: 'Delete',
    modifiers: {},
    category: 'library',
    scope: 'local',
    contexts: ['library'],
    enabled: true,
    defaultKey: 'Delete',
    defaultModifiers: {},
  },
];

// ============================================================================
// Input Button Mapping (for controller-like actions)
// ============================================================================

export const HOTKEY_TO_INPUT_BUTTON: Record<string, InputButton> = {
  'up': 'up',
  'down': 'down',
  'left': 'left',
  'right': 'right',
  'a': 'a',
  'b': 'b',
  'x': 'x',
  'y': 'y',
  'start': 'start',
  'select': 'select',
  'l': 'l',
  'r': 'r',
};
