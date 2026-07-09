/**
 * Event Bus Service
 * 
 * A type-safe event emitter for inter-component communication in the
 * OxideSFC frontend application.
 */

import type {
  EmulationEvents,
  LibraryEvents,
  SettingsEvents,
  InputEvents,
  VideoFrame,
  GameInfo,
  ScanProgress,
  ScanResult,
  InputButton,
  GamepadState
} from '../domain/types';

// ============================================================================
// Event Type Definitions
// ============================================================================

/**
 * Combined map of every event type in the application to its payload,
 * assembled from the domain-level per-category event maps.
 */
type AppEvents = EmulationEvents & LibraryEvents & SettingsEvents & InputEvents;

/**
 * All supported event types in the application
 */
export type EventType = keyof AppEvents;

/**
 * Event payload types
 */
export type EventPayload = {
  [T in EventType]: { type: T; payload: AppEvents[T] };
}[EventType];

// ============================================================================
// Event Handler Types
// ============================================================================

/**
 * Generic event handler function
 */
type EventHandler<T = unknown> = (data: T) => void;

// ============================================================================
// Event Bus Implementation
// ============================================================================

/**
 * Type-safe event bus for inter-component communication
 * 
 * Supports the following event categories:
 * - Emulation events: start, pause, resume, stop, frame
 * - Library events: scan:start, scan:progress, scan:complete
 * - Settings events: settings:change
 * - Input events: button, gamepad:connect, gamepad:disconnect
 */
class EventBusImpl {
  private handlers: Map<EventType, Set<EventHandler>> = new Map();
  private onceHandlers: Map<EventType, Set<EventHandler>> = new Map();
  private eventHistory: Array<{ type: EventType; data: unknown; timestamp: number }> = [];
  private maxHistorySize = 100;

  /**
   * Subscribe to an event
   * @param event - Event type to subscribe to
   * @param handler - Function to call when event is emitted
   * @returns Unsubscribe function
   */
  on<T extends EventType>(event: T, handler: EventHandler<ExtractPayload<T>>): () => void {
    if (!this.handlers.has(event)) {
      this.handlers.set(event, new Set());
    }
    
    this.handlers.get(event)!.add(handler as EventHandler);
    
    // Return unsubscribe function
    return () => {
      this.off(event, handler);
    };
  }

  /**
   * Subscribe to an event for one-time execution
   * @param event - Event type to subscribe to
   * @param handler - Function to call when event is emitted
   * @returns Unsubscribe function
   */
  once<T extends EventType>(event: T, handler: EventHandler<ExtractPayload<T>>): () => void {
    if (!this.onceHandlers.has(event)) {
      this.onceHandlers.set(event, new Set());
    }
    
    this.onceHandlers.get(event)!.add(handler as EventHandler);
    
    // Return unsubscribe function
    return () => {
      this.onceHandlers.get(event)?.delete(handler as EventHandler);
    };
  }

  /**
   * Unsubscribe from an event
   * @param event - Event type to unsubscribe from
   * @param handler - Handler function to remove
   */
  off<T extends EventType>(event: T, handler: EventHandler<ExtractPayload<T>>): void {
    this.handlers.get(event)?.delete(handler as EventHandler);
    this.onceHandlers.get(event)?.delete(handler as EventHandler);
  }

  /**
   * Emit an event to all subscribers
   * @param event - Event type to emit
   * @param data - Data to pass to handlers
   */
  emit<T extends EventType>(event: T, data: ExtractPayload<T>): void {
    // Add to history
    this.addToHistory(event, data);

    // Call regular handlers
    const handlers = this.handlers.get(event);
    if (handlers) {
      handlers.forEach(handler => {
        try {
          handler(data);
        } catch (error) {
          console.error(`Error in event handler for ${event}:`, error);
        }
      });
    }

    // Call once handlers and then remove them
    const onceHandlers = this.onceHandlers.get(event);
    if (onceHandlers) {
      onceHandlers.forEach(handler => {
        try {
          handler(data);
        } catch (error) {
          console.error(`Error in once handler for ${event}:`, error);
        }
      });
      this.onceHandlers.delete(event);
    }
  }

  /**
   * Clear all handlers for an event
   * @param event - Event type to clear
   */
  clear(event?: EventType): void {
    if (event) {
      this.handlers.delete(event);
      this.onceHandlers.delete(event);
    } else {
      this.handlers.clear();
      this.onceHandlers.clear();
    }
  }

  /**
   * Get all handlers for an event
   * @param event - Event type to check
   * @returns Number of handlers subscribed
   */
  listenerCount(event: EventType): number {
    const regular = this.handlers.get(event)?.size ?? 0;
    const once = this.onceHandlers.get(event)?.size ?? 0;
    return regular + once;
  }

  /**
   * Check if event has subscribers
   * @param event - Event type to check
   * @returns true if event has subscribers
   */
  hasListeners(event: EventType): boolean {
    return this.listenerCount(event) > 0;
  }

  /**
   * Get event history
   * @param event - Optional event type to filter by
   * @param limit - Maximum number of events to return
   * @returns Array of historical events
   */
  getHistory(event?: EventType, limit = 50): Array<{ type: EventType; data: unknown; timestamp: number }> {
    let history = this.eventHistory;
    
    if (event) {
      history = history.filter(e => e.type === event);
    }
    
    return history.slice(-limit);
  }

  /**
   * Clear event history
   */
  clearHistory(): void {
    this.eventHistory = [];
  }

  /**
   * Add event to history
   */
  private addToHistory(event: EventType, data: unknown): void {
    this.eventHistory.push({
      type: event,
      data,
      timestamp: Date.now(),
    });

    // Trim history if too large
    if (this.eventHistory.length > this.maxHistorySize) {
      this.eventHistory = this.eventHistory.slice(-this.maxHistorySize);
    }
  }
}

// ============================================================================
// Type Helpers
// ============================================================================

/**
 * Extract payload type from event type
 */
type ExtractPayload<T extends EventType> = AppEvents[T];

// ============================================================================
// Convenience Methods for Common Events
// ============================================================================

/**
 * Event bus singleton instance
 */
export const eventBus = new EventBusImpl();

// ============================================================================
// Convenience Methods
// ============================================================================

/**
 * Emit emulation start event
 */
export function emitEmulationStart(game: GameInfo): void {
  eventBus.emit('emulation:start', { game });
}

/**
 * Emit emulation pause event
 */
export function emitEmulationPause(): void {
  eventBus.emit('emulation:pause', undefined);
}

/**
 * Emit emulation resume event
 */
export function emitEmulationResume(): void {
  eventBus.emit('emulation:resume', undefined);
}

/**
 * Emit emulation stop event
 */
export function emitEmulationStop(): void {
  eventBus.emit('emulation:stop', undefined);
}

/**
 * Emit emulation frame event
 */
export function emitEmulationFrame(frame: VideoFrame): void {
  eventBus.emit('emulation:frame', { frame });
}

/**
 * Emit library scan start event
 */
export function emitLibraryScanStart(directories: string[]): void {
  eventBus.emit('library:scan:start', { directories });
}

/**
 * Emit library scan progress event
 */
export function emitLibraryScanProgress(progress: ScanProgress): void {
  eventBus.emit('library:scan:progress', { progress });
}

/**
 * Emit library scan complete event
 */
export function emitLibraryScanComplete(result: ScanResult): void {
  eventBus.emit('library:scan:complete', { result });
}

/**
 * Emit settings change event
 */
export function emitSettingsChange(key: string, value: unknown, previousValue: unknown): void {
  eventBus.emit('settings:change', { key, value, previousValue });
}

/**
 * Emit input button event
 */
export function emitInputButton(button: InputButton, pressed: boolean): void {
  eventBus.emit('input:button', { button, pressed });
}

/**
 * Emit gamepad connect event
 */
export function emitGamepadConnect(gamepad: GamepadState): void {
  eventBus.emit('input:gamepad:connect', { gamepad });
}

/**
 * Emit gamepad disconnect event
 */
export function emitGamepadDisconnect(index: number): void {
  eventBus.emit('input:gamepad:disconnect', { index });
}

// ============================================================================
// Re-export types (for external use)
// ============================================================================
// Note: EventType, EventHandler are already exported via class methods
