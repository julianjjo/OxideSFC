/**
 * Command Manager Service
 * 
 * Provides queue and execution management for commands with support
 * for undo/redo operations and command history tracking.
 */

import type { Command, CommandHistoryEntry } from '../domain/types';

// ============================================================================
// Command Manager Types
// ============================================================================

/**
 * Command execution result
 */
export interface CommandResult {
  success: boolean;
  error?: string;
  timestamp: number;
}

/**
 * Command queue item
 */
interface QueueItem {
  command: Command;
  resolve: (result: CommandResult) => void;
  reject: (error: Error) => void;
  timeout?: ReturnType<typeof setTimeout>;
}

/**
 * Command Manager configuration
 */
export interface CommandManagerConfig {
  maxHistorySize: number;
  maxQueueSize: number;
  defaultTimeout: number;
  enableLogging: boolean;
}

/**
 * Command Manager state
 */
interface CommandManagerState {
  isExecuting: boolean;
  history: CommandHistoryEntry[];
  undoStack: Command[];
  redoStack: Command[];
  queue: QueueItem[];
}

// ============================================================================
// Default Configuration
// ============================================================================

const DEFAULT_CONFIG: CommandManagerConfig = {
  maxHistorySize: 100,
  maxQueueSize: 50,
  defaultTimeout: 30000, // 30 seconds
  enableLogging: false,
};

// ============================================================================
// Command Manager Implementation
// ============================================================================

/**
 * Command Manager for queueing and executing commands with undo/redo support
 */
class CommandManagerImpl {
  private config: CommandManagerConfig;
  private state: CommandManagerState;
  private eventHandlers: Map<string, Set<(data: unknown) => void>> = new Map();

  constructor(config: Partial<CommandManagerConfig> = {}) {
    this.config = { ...DEFAULT_CONFIG, ...config };
    this.state = {
      isExecuting: false,
      history: [],
      undoStack: [],
      redoStack: [],
      queue: [],
    };
  }

  // ==========================================================================
  // Configuration
  // ==========================================================================

  /**
   * Update configuration
   */
  configure(config: Partial<CommandManagerConfig>): void {
    this.config = { ...this.config, ...config };
  }

  /**
   * Get current configuration
   */
  getConfig(): CommandManagerConfig {
    return { ...this.config };
  }

  // ==========================================================================
  // Command Execution
  // ==========================================================================

  /**
   * Execute a command immediately
   * @param command - Command to execute
   * @returns Promise that resolves with the result
   */
  async execute(command: Command): Promise<CommandResult> {
    const startTime = Date.now();
    
    try {
      this.log(`Executing command: ${command.type} - ${command.description}`);
      
      // Execute the command
      await command.execute();
      
      // Add to history
      this.addToHistory(command);
      
      // Add to undo stack
      this.state.undoStack.push(command);
      
      // Clear redo stack when new command is executed
      this.state.redoStack = [];
      
      const duration = Date.now() - startTime;
      this.log(`Command executed successfully in ${duration}ms`);
      
      // Emit command executed event
      this.emit('command:executed', { command, duration, success: true });
      
      return {
        success: true,
        timestamp: Date.now(),
      };
    } catch (error) {
      const duration = Date.now() - startTime;
      const errorMessage = error instanceof Error ? error.message : String(error);
      
      this.log(`Command failed after ${duration}ms: ${errorMessage}`, 'error');
      
      // Emit command failed event
      this.emit('command:failed', { command, duration, error: errorMessage });
      
      return {
        success: false,
        error: errorMessage,
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Queue a command for execution
   * @param command - Command to queue
   * @returns Promise that resolves with the result
   */
  queue(command: Command): Promise<CommandResult> {
    return new Promise((resolve, reject) => {
      // Check queue size
      if (this.state.queue.length >= this.config.maxQueueSize) {
        reject(new Error('Command queue is full'));
        return;
      }

      // Create queue item
      const timeout = setTimeout(() => {
        // Remove from queue on timeout
        const index = this.state.queue.findIndex(item => item.command.id === command.id);
        if (index !== -1) {
          this.state.queue.splice(index, 1);
        }
        
        reject(new Error('Command execution timed out'));
      }, this.config.defaultTimeout);

      const item: QueueItem = {
        command,
        resolve,
        reject,
        timeout,
      };

      this.state.queue.push(item);
      this.log(`Command queued: ${command.type}`);

      // Start processing if not already
      this.processQueue();
    });
  }

  /**
   * Process the command queue
   */
  private async processQueue(): Promise<void> {
    // Skip if already executing or queue is empty
    if (this.state.isExecuting || this.state.queue.length === 0) {
      return;
    }

    this.state.isExecuting = true;
    this.emit('queue:start', { queueSize: this.state.queue.length });

    while (this.state.queue.length > 0) {
      const item = this.state.queue.shift()!;
      
      // Clear timeout
      if (item.timeout) {
        clearTimeout(item.timeout);
      }

      try {
        const result = await this.execute(item.command);
        item.resolve(result);
      } catch (error) {
        item.reject(error instanceof Error ? error : new Error(String(error)));
      }
    }

    this.state.isExecuting = false;
    this.emit('queue:end', {});
  }

  /**
   * Clear the command queue
   */
  clearQueue(): void {
    // Clear all timeouts
    for (const item of this.state.queue) {
      if (item.timeout) {
        clearTimeout(item.timeout);
      }
    }
    
    this.state.queue = [];
    this.emit('queue:clear', {});
  }

  // ==========================================================================
  // Undo/Redo Operations
  // ==========================================================================

  /**
   * Undo the last command
   * @returns Promise that resolves with the result
   */
  async undo(): Promise<CommandResult> {
    const command = this.state.undoStack.pop();
    
    if (!command) {
      return {
        success: false,
        error: 'Nothing to undo',
        timestamp: Date.now(),
      };
    }

    this.log(`Undoing command: ${command.type}`);

    try {
      await command.undo();
      this.state.redoStack.push(command);
      
      // Remove from history (last entry)
      if (this.state.history.length > 0) {
        this.state.history.pop();
      }
      
      this.emit('command:undone', { command });
      
      return {
        success: true,
        timestamp: Date.now(),
      };
    } catch (error) {
      // Put command back on undo stack if undo fails
      this.state.undoStack.push(command);
      
      const errorMessage = error instanceof Error ? error.message : String(error);
      this.emit('command:undo-failed', { command, error: errorMessage });
      
      return {
        success: false,
        error: errorMessage,
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Redo the last undone command
   * @returns Promise that resolves with the result
   */
  async redo(): Promise<CommandResult> {
    const command = this.state.redoStack.pop();
    
    if (!command) {
      return {
        success: false,
        error: 'Nothing to redo',
        timestamp: Date.now(),
      };
    }

    this.log(`Redoing command: ${command.type}`);

    try {
      await command.execute();
      this.state.undoStack.push(command);
      this.addToHistory(command);
      
      this.emit('command:redone', { command });
      
      return {
        success: true,
        timestamp: Date.now(),
      };
    } catch (error) {
      // Put command back on redo stack if redo fails
      this.state.redoStack.push(command);
      
      const errorMessage = error instanceof Error ? error.message : String(error);
      this.emit('command:redo-failed', { command, error: errorMessage });
      
      return {
        success: false,
        error: errorMessage,
        timestamp: Date.now(),
      };
    }
  }

  /**
   * Check if undo is available
   */
  canUndo(): boolean {
    return this.state.undoStack.length > 0;
  }

  /**
   * Check if redo is available
   */
  canRedo(): boolean {
    return this.state.redoStack.length > 0;
  }

  // ==========================================================================
  // History Management
  // ==========================================================================

  /**
   * Add command to history
   */
  private addToHistory(command: Command): void {
    const entry: CommandHistoryEntry = {
      command,
      executedAt: Date.now(),
    };

    this.state.history.push(entry);

    // Trim history if too large
    if (this.state.history.length > this.config.maxHistorySize) {
      this.state.history = this.state.history.slice(-this.config.maxHistorySize);
    }

    this.emit('history:add', entry);
  }

  /**
   * Get command history
   * @param limit - Maximum number of entries to return
   * @returns Array of history entries
   */
  getHistory(limit?: number): CommandHistoryEntry[] {
    if (limit) {
      return this.state.history.slice(-limit);
    }
    return [...this.state.history];
  }

  /**
   * Clear command history
   */
  clearHistory(): void {
    this.state.history = [];
    this.state.undoStack = [];
    this.state.redoStack = [];
    this.emit('history:clear', {});
  }

  // ==========================================================================
  // State Queries
  // ==========================================================================

  /**
   * Check if a command is currently executing
   */
  isExecuting(): boolean {
    return this.state.isExecuting;
  }

  /**
   * Get current queue size
   */
  getQueueSize(): number {
    return this.state.queue.length;
  }

  /**
   * Get undo stack size
   */
  getUndoStackSize(): number {
    return this.state.undoStack.length;
  }

  /**
   * Get redo stack size
   */
  getRedoStackSize(): number {
    return this.state.redoStack.length;
  }

  // ==========================================================================
  // Event System
  // ==========================================================================

  /**
   * Subscribe to command manager events
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
          console.error(`Error in CommandManager event handler for ${event}:`, error);
        }
      });
    }
  }

  // ==========================================================================
  // Logging
  // ==========================================================================

  private log(message: string, level: 'info' | 'error' = 'info'): void {
    if (this.config.enableLogging) {
      const prefix = '[CommandManager]';
      if (level === 'error') {
        console.error(`${prefix} ${message}`);
      } else {
        console.log(`${prefix} ${message}`);
      }
    }
  }
}

// ============================================================================
// Factory Functions
// ============================================================================

/**
 * Create a new command with unique ID
 */
export function createCommand(
  type: string,
  description: string,
  execute: () => Promise<void> | void,
  undo: () => Promise<void> | void
): Command {
  return {
    id: `${type}-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`,
    type,
    description,
    execute,
    undo,
    timestamp: Date.now(),
  };
}

/**
 * Create an async command
 */
export function createAsyncCommand<T>(
  type: string,
  description: string,
  execute: () => Promise<T>,
  undo: () => Promise<void> | void,
  onSuccess?: (result: T) => void,
  onError?: (error: Error) => void
): Command {
  return {
    id: `${type}-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`,
    type,
    description,
    execute: async () => {
      try {
        const result = await execute();
        onSuccess?.(result);
      } catch (error) {
        onError?.(error instanceof Error ? error : new Error(String(error)));
        throw error;
      }
    },
    undo,
    timestamp: Date.now(),
  };
}

// ============================================================================
// Singleton Instance
// ============================================================================

export const commandManager = new CommandManagerImpl();

// ============================================================================
// Export Types
// ============================================================================
// Note: CommandManagerConfig and CommandResult are already exported via class methods/interface
