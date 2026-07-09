/**
 * Hotkey Service
 * 
 * Manages keyboard shortcuts for the application with support for:
 * - Global hotkeys (work even when emulator is focused)
 * - Local hotkeys (only work in specific contexts)
 * - Hotkey categories: General, Emulation, Library, Navigation
 * - Customizable key combinations
 * - Conflict detection and prevention
 */

import type {
  HotkeyBinding,
  HotkeyCategory,
  HotkeyEvent,
  HotkeyConflict,
  HotkeyModifiers,
  HotkeyScope,
} from './types';
import { DEFAULT_HOTKEYS } from './types';

type HotkeyHandler = (event: HotkeyEvent) => void;

// ============================================================================
// Service State
// ============================================================================

interface HotkeyServiceState {
  bindings: Map<string, HotkeyBinding>;
  handlers: Map<string, Set<HotkeyHandler>>;
  currentContext: string;
  isEnabled: boolean;
  isListening: boolean;
}

// ============================================================================
// Hotkey Service Implementation
// ============================================================================

class HotkeyServiceImpl {
  private state: HotkeyServiceState = {
    bindings: new Map(),
    handlers: new Map(),
    currentContext: 'library',
    isEnabled: true,
    isListening: false,
  };

  private keydownHandler: ((e: KeyboardEvent) => void) | null = null;
  private keyupHandler: ((e: KeyboardEvent) => void) | null = null;

  constructor() {
    this.initializeDefaultBindings();
  }

  /**
   * Initialize with default hotkey bindings
   */
  private initializeDefaultBindings(): void {
    DEFAULT_HOTKEYS.forEach(binding => {
      this.state.bindings.set(binding.id, binding);
    });
  }

  // ============================================================================
  // Binding Management
  // ============================================================================

  /**
   * Get all registered hotkey bindings
   */
  getBindings(): HotkeyBinding[] {
    return Array.from(this.state.bindings.values());
  }

  /**
   * Get bindings by category
   */
  getBindingsByCategory(category: HotkeyCategory): HotkeyBinding[] {
    return this.getBindings().filter(b => b.category === category);
  }

  /**
   * Get bindings by scope
   */
  getBindingsByScope(scope: HotkeyScope): HotkeyBinding[] {
    return this.getBindings().filter(b => b.scope === scope);
  }

  /**
   * Get a specific binding by ID
   */
  getBinding(id: string): HotkeyBinding | undefined {
    return this.state.bindings.get(id);
  }

  /**
   * Get a binding by action
   */
  getBindingByAction(action: string): HotkeyBinding | undefined {
    return this.getBindings().find(b => b.action === action);
  }

  /**
   * Register a new hotkey binding
   */
  registerBinding(binding: HotkeyBinding): boolean {
    // Check for conflicts
    const conflicts = this.detectConflict(binding);
    if (conflicts.length > 0) {
      console.warn(`Hotkey conflict detected for "${binding.name}":`, conflicts.map(c => c.description));
      // Still allow registration but warn
    }

    this.state.bindings.set(binding.id, binding);
    return true;
  }

  /**
   * Unregister a hotkey binding
   */
  unregisterBinding(id: string): boolean {
    return this.state.bindings.delete(id);
  }

  /**
   * Update an existing binding
   */
  updateBinding(id: string, updates: Partial<HotkeyBinding>): boolean {
    const binding = this.state.bindings.get(id);
    if (!binding) return false;

    const updated = { ...binding, ...updates };
    
    // Check for conflicts with the updated binding
    const conflicts = this.detectConflict(updated);
    if (conflicts.length > 0) {
      console.warn(`Hotkey conflict detected after update:`, conflicts.map(c => c.description));
    }

    this.state.bindings.set(id, updated);
    return true;
  }

  /**
   * Enable or disable a specific binding
   */
  setBindingEnabled(id: string, enabled: boolean): boolean {
    return this.updateBinding(id, { enabled });
  }

  /**
   * Reset a binding to its defaults
   */
  resetBinding(id: string): boolean {
    const binding = this.state.bindings.get(id);
    if (!binding || !binding.defaultKey) return false;

    return this.updateBinding(id, {
      key: binding.defaultKey,
      modifiers: binding.defaultModifiers || {},
    });
  }

  /**
   * Reset all bindings to defaults
   */
  resetAllBindings(): void {
    this.state.bindings.clear();
    DEFAULT_HOTKEYS.forEach(binding => {
      this.state.bindings.set(binding.id, binding);
    });
  }

  // ============================================================================
  // Handler Management
  // ============================================================================

  /**
   * Register a handler for a hotkey action
   */
  onAction(action: string, handler: HotkeyHandler): () => void {
    if (!this.state.handlers.has(action)) {
      this.state.handlers.set(action, new Set());
    }
    this.state.handlers.get(action)!.add(handler);

    // Return unsubscribe function
    return () => {
      this.state.handlers.get(action)?.delete(handler);
    };
  }

  /**
   * Register a handler for multiple actions
   */
  onActions(actions: string[], handler: HotkeyHandler): () => void {
    const unsubscribes = actions.map(action => this.onAction(action, handler));
    return () => unsubscribes.forEach(unsub => unsub());
  }

  // ============================================================================
  // Context Management
  // ============================================================================

  /**
   * Set the current context for local hotkeys
   */
  setContext(context: string): void {
    this.state.currentContext = context;
  }

  /**
   * Get the current context
   */
  getContext(): string {
    return this.state.currentContext;
  }

  // ============================================================================
  // Conflict Detection
  // ============================================================================

  /**
   * Detect conflicts between bindings
   */
  detectConflict(newBinding: HotkeyBinding): HotkeyConflict[] {
    const conflicts: HotkeyConflict[] = [];
    const newKeyCombo = this.getKeyCombo(newBinding.key, newBinding.modifiers);

    for (const existing of this.state.bindings.values()) {
      if (existing.id === newBinding.id) continue;
      if (!existing.enabled) continue;

      const existingKeyCombo = this.getKeyCombo(existing.key, existing.modifiers);
      
      if (newKeyCombo === existingKeyCombo) {
        conflicts.push({
          binding1: newBinding,
          binding2: existing,
          description: `"${newBinding.name}" conflicts with "${existing.name}"`,
        });
      }
    }

    return conflicts;
  }

  /**
   * Check if a key combination is already in use
   */
  isKeyComboInUse(key: string, modifiers: HotkeyModifiers, excludeId?: string): HotkeyBinding | null {
    const keyCombo = this.getKeyCombo(key, modifiers);
    
    for (const binding of this.state.bindings.values()) {
      if (excludeId && binding.id === excludeId) continue;
      if (!binding.enabled) continue;

      const existingCombo = this.getKeyCombo(binding.key, binding.modifiers);
      if (keyCombo === existingCombo) {
        return binding;
      }
    }

    return null;
  }

  /**
   * Generate a unique key combo string
   */
  private getKeyCombo(key: string, modifiers: HotkeyModifiers): string {
    const parts: string[] = [];
    if (modifiers.ctrl) parts.push('Ctrl');
    if (modifiers.alt) parts.push('Alt');
    if (modifiers.shift) parts.push('Shift');
    if (modifiers.meta) parts.push('Meta');
    parts.push(key);
    return parts.join('+');
  }

  // ============================================================================
  // Event Handling
  // ============================================================================

  /**
   * Start listening for keyboard events
   */
  startListening(): void {
    if (this.state.isListening) return;

    this.keydownHandler = this.handleKeyDown.bind(this);
    window.addEventListener('keydown', this.keydownHandler);
    
    this.keyupHandler = this.handleKeyUp.bind(this);
    window.addEventListener('keyup', this.keyupHandler);

    this.state.isListening = true;
  }

  /**
   * Stop listening for keyboard events
   */
  stopListening(): void {
    if (!this.state.isListening) return;

    if (this.keydownHandler) {
      window.removeEventListener('keydown', this.keydownHandler);
      this.keydownHandler = null;
    }
    
    if (this.keyupHandler) {
      window.removeEventListener('keyup', this.keyupHandler);
      this.keyupHandler = null;
    }

    this.state.isListening = false;
  }

  /**
   * Enable or disable the hotkey service
   */
  setEnabled(enabled: boolean): void {
    this.state.isEnabled = enabled;
    if (!enabled) {
      this.stopListening();
    }
  }

  /**
   * Check if the service is enabled
   */
  isEnabled(): boolean {
    return this.state.isEnabled;
  }

  /**
   * Handle keydown events
   */
  private handleKeyDown(event: KeyboardEvent): void {
    if (!this.state.isEnabled) return;

    // Don't capture if user is typing in an input
    const target = event.target as HTMLElement;
    if (target.tagName === 'INPUT' || 
        target.tagName === 'TEXTAREA' || 
        target.isContentEditable) {
      return;
    }

    const key = event.code;
    const modifiers: HotkeyModifiers = {
      ctrl: event.ctrlKey,
      alt: event.altKey,
      shift: event.shiftKey,
      meta: event.metaKey,
    };

    // Find matching binding
    const binding = this.findMatchingBinding(key, modifiers);
    if (!binding || !binding.enabled) return;

    // Check context for local hotkeys
    if (binding.scope === 'local' && binding.contexts) {
      if (!binding.contexts.includes(this.state.currentContext)) {
        return;
      }
    }

    // Prevent default for most hotkeys (except Escape in some cases)
    if (binding.action !== 'exit' || key !== 'Escape') {
      event.preventDefault();
    }

    // Create event object
    const hotkeyEvent: HotkeyEvent = {
      action: binding.action,
      key: binding.key,
      modifiers: binding.modifiers,
      timestamp: Date.now(),
    };

    // Call registered handlers
    const handlers = this.state.handlers.get(binding.action);
    if (handlers) {
      handlers.forEach(handler => {
        try {
          handler(hotkeyEvent);
        } catch (err) {
          console.error(`Error in hotkey handler for "${binding.action}":`, err);
        }
      });
    }
  }

  /**
   * Handle keyup events (for modifier-only actions)
   */
  private handleKeyUp(_event: KeyboardEvent): void {
    // Currently not used, but available for future features
  }

  /**
   * Find a binding that matches the given key and modifiers
   */
  private findMatchingBinding(key: string, modifiers: HotkeyModifiers): HotkeyBinding | null {
    for (const binding of this.state.bindings.values()) {
      if (binding.key !== key) continue;
      
      // Check modifiers match
      const bindingMods = binding.modifiers;
      if ((bindingMods.ctrl || false) !== (modifiers.ctrl || false)) continue;
      if ((bindingMods.alt || false) !== (modifiers.alt || false)) continue;
      if ((bindingMods.shift || false) !== (modifiers.shift || false)) continue;
      if ((bindingMods.meta || false) !== (modifiers.meta || false)) continue;
      
      return binding;
    }

    return null;
  }

  // ============================================================================
  // Utility Methods
  // ============================================================================

  /**
   * Get a human-readable string for a key combination
   */
  formatKeyCombo(key: string, modifiers: HotkeyModifiers): string {
    const parts: string[] = [];
    if (modifiers.ctrl) parts.push('Ctrl');
    if (modifiers.alt) parts.push('Alt');
    if (modifiers.shift) parts.push('Shift');
    if (modifiers.meta) parts.push('Meta');
    
    // Format the key
    let keyPart = key
      .replace('Key', '')
      .replace('Digit', '')
      .replace('Arrow', '')
      .replace('Left', '←')
      .replace('Right', '→')
      .replace('Up', '↑')
      .replace('Down', '↓')
      .replace('Escape', 'Esc')
      .replace('Delete', 'Del')
      .replace('Backspace', 'Back')
      .replace('Space', 'Space');
    
    parts.push(keyPart);
    return parts.join('+');
  }

  /**
   * Get category display name
   */
  getCategoryLabel(category: HotkeyCategory): string {
    const labels: Record<HotkeyCategory, string> = {
      general: 'General',
      emulation: 'Emulation',
      library: 'Library',
      navigation: 'Navigation',
    };
    return labels[category];
  }

  /**
   * Export bindings as JSON
   */
  exportBindings(): string {
    const bindings = this.getBindings();
    return JSON.stringify(bindings, null, 2);
  }

  /**
   * Import bindings from JSON
   */
  importBindings(json: string): boolean {
    try {
      const bindings: HotkeyBinding[] = JSON.parse(json);
      
      // Validate and merge
      bindings.forEach(binding => {
        if (binding.id && binding.action && binding.key) {
          this.state.bindings.set(binding.id, binding);
        }
      });
      
      return true;
    } catch (error) {
      console.error('Failed to import hotkey bindings:', error);
      return false;
    }
  }
}

// ============================================================================
// Singleton Instance
// ============================================================================

export const HotkeyService = new HotkeyServiceImpl();

// ============================================================================
// Re-export Types
// ============================================================================

export type {
  HotkeyBinding,
  HotkeyCategory,
  HotkeyEvent,
  HotkeyConflict,
  HotkeyModifiers,
  HotkeyScope,
};
