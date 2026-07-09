/**
 * useKeyboard Hook
 * 
 * React hook for handling keyboard input with support for:
 * - Key down/up event handling
 * - SNES button mapping from domain types
 * - Key remapping from settings
 * - Modifier key handling (Shift, Ctrl, Alt)
 * - Configurable repeat delay and rate
 * - Event emission via eventBus
 */

import { useEffect, useRef, useCallback, useState } from 'react';
import type { InputButton, KeyboardMapping } from '../domain/types';
import { emitInputButton } from '../services/eventBus';

// ============================================================================
// Configuration Types
// ============================================================================

export interface KeyboardHookConfig {
  /** Enable keyboard input handling */
  enabled?: boolean;
  /** Custom keyboard mapping (overrides default) */
  mapping?: KeyboardMapping;
  /** Initial repeat delay in ms (before repeating starts) */
  repeatDelay?: number;
  /** Repeat rate in ms (between repeats) */
  repeatRate?: number;
  /** Prevent default browser behavior for mapped keys */
  preventDefault?: boolean;
  /** Callback when button state changes */
  onButtonChange?: (button: InputButton, pressed: boolean) => void;
}

// ============================================================================
// Modifier Key State
// ============================================================================

export interface ModifierKeys {
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
  meta: boolean;
}

// ============================================================================
// Default SNES Keyboard Mapping
// ============================================================================

export const DEFAULT_SNES_KEYBOARD_MAPPING: KeyboardMapping = {
  'ArrowUp': 'up',
  'ArrowDown': 'down',
  'ArrowLeft': 'left',
  'ArrowRight': 'right',
  'KeyZ': 'a',
  'KeyX': 'b',
  'KeyA': 'x',
  'KeyS': 'y',
  'Enter': 'start',
  'ShiftRight': 'select',
  'KeyQ': 'l',
  'KeyW': 'r',
};

// ============================================================================
// Hook Implementation
// ============================================================================

/**
 * useKeyboard - Keyboard input hook for SNES emulation
 * 
 * @param config - Configuration options for the hook
 * @returns Object containing keyboard state and utilities
 * 
 * @example
 * ```tsx
 * const { 
 *   pressedButtons, 
 *   modifiers, 
 *   remapKey, 
 *   resetMapping 
 * } = useKeyboard({
 *   enabled: true,
 *   mapping: customMapping,
 *   repeatDelay: 250,
 *   repeatRate: 50,
 *   onButtonChange: (button, pressed) => console.log(button, pressed)
 * });
 * ```
 */
export function useKeyboard(config: KeyboardHookConfig = {}) {
  const {
    enabled = true,
    mapping: initialMapping,
    repeatDelay = 250,
    repeatRate = 50,
    preventDefault = true,
    onButtonChange,
  } = config;

  // State
  const [pressedButtons, setPressedButtons] = useState<Set<InputButton>>(new Set());
  const [modifiers, setModifiers] = useState<ModifierKeys>({
    shift: false,
    ctrl: false,
    alt: false,
    meta: false,
  });

  // Refs
  const mappingRef = useRef<KeyboardMapping>(initialMapping ?? DEFAULT_SNES_KEYBOARD_MAPPING);
  const pressedKeysRef = useRef<Set<string>>(new Set());
  const repeatTimersRef = useRef<Map<string, number>>(new Map());
  const repeatDelaysRef = useRef<Map<string, number>>(new Map());

  // Update mapping when prop changes
  useEffect(() => {
    if (initialMapping) {
      mappingRef.current = initialMapping;
    }
  }, [initialMapping]);

  // Handle key down
  const handleKeyDown = useCallback((event: KeyboardEvent) => {
    if (!enabled) return;

    const keyCode = event.code;
    const button = mappingRef.current[keyCode];

    // Update modifier state
    if (event.shiftKey || event.key === 'Shift') {
      setModifiers(prev => ({ ...prev, shift: true }));
    }
    if (event.ctrlKey || event.key === 'Control') {
      setModifiers(prev => ({ ...prev, ctrl: true }));
    }
    if (event.altKey || event.key === 'Alt') {
      setModifiers(prev => ({ ...prev, alt: true }));
    }
    if (event.metaKey || event.key === 'Meta') {
      setModifiers(prev => ({ ...prev, meta: true }));
    }

    // If this key is mapped to a button
    if (button) {
      // Prevent default if configured
      if (preventDefault) {
        event.preventDefault();
      }

      // If not already pressed, handle initial press
      if (!pressedKeysRef.current.has(keyCode)) {
        pressedKeysRef.current.add(keyCode);
        
        // Update button state
        setPressedButtons(prev => {
          const newSet = new Set(prev);
          newSet.add(button);
          return newSet;
        });

        // Emit button event
        emitInputButton(button, true);
        
        // Call callback
        onButtonChange?.(button, true);

        // Set up repeat timer
        const startRepeat = () => {
          const timerId = window.setInterval(() => {
            emitInputButton(button, true);
            onButtonChange?.(button, true);
          }, repeatRate);
          
          repeatTimersRef.current.set(keyCode, timerId);
        };

        // Delay before starting repeat
        const delayTimerId = window.setTimeout(startRepeat, repeatDelay);
        repeatDelaysRef.current.set(keyCode, delayTimerId);
      }
    }
  }, [enabled, preventDefault, repeatDelay, repeatRate, onButtonChange]);

  // Handle key up
  const handleKeyUp = useCallback((event: KeyboardEvent) => {
    if (!enabled) return;

    const keyCode = event.code;
    const button = mappingRef.current[keyCode];

    // Update modifier state
    if (!event.shiftKey && event.key !== 'Shift') {
      setModifiers(prev => ({ ...prev, shift: false }));
    }
    if (!event.ctrlKey && event.key !== 'Control') {
      setModifiers(prev => ({ ...prev, ctrl: false }));
    }
    if (!event.altKey && event.key !== 'Alt') {
      setModifiers(prev => ({ ...prev, alt: false }));
    }
    if (!event.metaKey && event.key !== 'Meta') {
      setModifiers(prev => ({ ...prev, meta: false }));
    }

    // If this key is mapped to a button
    if (button) {
      // Prevent default if configured
      if (preventDefault) {
        event.preventDefault();
      }

      // Clear pressed state
      pressedKeysRef.current.delete(keyCode);

      // Clear repeat timers
      const delayTimer = repeatDelaysRef.current.get(keyCode);
      if (delayTimer) {
        clearTimeout(delayTimer);
        repeatDelaysRef.current.delete(keyCode);
      }

      const repeatTimer = repeatTimersRef.current.get(keyCode);
      if (repeatTimer) {
        clearInterval(repeatTimer);
        repeatTimersRef.current.delete(keyCode);
      }

      // Update button state
      setPressedButtons(prev => {
        const newSet = new Set(prev);
        newSet.delete(button);
        return newSet;
      });

      // Emit button release event
      emitInputButton(button, false);
      
      // Call callback
      onButtonChange?.(button, false);
    }
  }, [enabled, preventDefault, onButtonChange]);

  // Set up event listeners
  useEffect(() => {
    if (!enabled) return;

    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);

    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('keyup', handleKeyUp);

      // Clean up timers
      repeatTimersRef.current.forEach(timer => clearInterval(timer));
      repeatDelaysRef.current.forEach(timer => clearTimeout(timer));
      repeatTimersRef.current.clear();
      repeatDelaysRef.current.clear();
    };
  }, [enabled, handleKeyDown, handleKeyUp]);

  // Remap a key
  const remapKey = useCallback((keyCode: string, button: InputButton | null) => {
    if (button === null) {
      // Remove mapping
      const newMapping = { ...mappingRef.current };
      delete newMapping[keyCode];
      mappingRef.current = newMapping;
    } else {
      // Set new mapping
      mappingRef.current = {
        ...mappingRef.current,
        [keyCode]: button,
      };
    }
  }, []);

  // Reset to default mapping
  const resetMapping = useCallback(() => {
    mappingRef.current = { ...DEFAULT_SNES_KEYBOARD_MAPPING };
  }, []);

  // Get current mapping
  const getMapping = useCallback(() => {
    return { ...mappingRef.current };
  }, []);

  // Check if a specific button is pressed
  const isButtonPressed = useCallback((button: InputButton): boolean => {
    return pressedButtons.has(button);
  }, [pressedButtons]);

  // Get all pressed buttons as array
  const getPressedButtons = useCallback((): InputButton[] => {
    return Array.from(pressedButtons);
  }, [pressedButtons]);

  // Force release all buttons (useful for focus loss)
  const releaseAll = useCallback(() => {
    pressedKeysRef.current.forEach(keyCode => {
      const button = mappingRef.current[keyCode];
      if (button) {
        emitInputButton(button, false);
        onButtonChange?.(button, false);
      }
    });

    pressedKeysRef.current.clear();
    
    // Clear all timers
    repeatTimersRef.current.forEach(timer => clearInterval(timer));
    repeatDelaysRef.current.forEach(timer => clearTimeout(timer));
    repeatTimersRef.current.clear();
    repeatDelaysRef.current.clear();

    setPressedButtons(new Set());
  }, [onButtonChange]);

  return {
    // State
    pressedButtons,
    modifiers,
    
    // Methods
    remapKey,
    resetMapping,
    getMapping,
    isButtonPressed,
    getPressedButtons,
    releaseAll,
  };
}

// ============================================================================
// Type Exports
// ============================================================================

export type { KeyboardMapping };
