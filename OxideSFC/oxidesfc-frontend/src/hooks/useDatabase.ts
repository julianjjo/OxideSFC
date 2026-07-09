/**
 * useDatabase Hook
 * 
 * React hook for managing local database operations using IndexedDB.
 * Provides CRUD operations for games, settings, save states, and more.
 * Includes automatic migrations and error handling.
 */

import { useEffect, useRef, useCallback, useState } from 'react';
import type { Game, AppSettings, ControllerProfile } from '../domain/types';

// ============================================================================
// Database Types
// ============================================================================

export interface DatabaseConfig {
  /** Database name */
  name?: string;
  /** Database version */
  version?: number;
  /** Enable debug logging */
  debug?: boolean;
}

/**
 * Database schema version
 */
const DB_VERSION = 1;
const DB_NAME = 'oxidesfc-db';

/**
 * Object store names
 */
const STORES = {
  GAMES: 'games',
  SETTINGS: 'settings',
  SAVE_STATES: 'saveStates',
  CONTROLLER_PROFILES: 'controllerProfiles',
  GAME_METADATA: 'gameMetadata',
} as const;

/**
 * Database state
 */
export interface DatabaseState {
  isReady: boolean;
  isLoading: boolean;
  error: Error | null;
}

// ============================================================================
// Database Error
// ============================================================================

export class DatabaseError extends Error {
  constructor(
    message: string,
    public readonly code?: string,
    public readonly originalError?: Error
  ) {
    super(message);
    this.name = 'DatabaseError';
  }
}

// ============================================================================
// Migration Functions
// ============================================================================

type MigrationFn = (db: IDBDatabase) => void;

const migrations: Record<number, MigrationFn> = {
  1: (db: IDBDatabase) => {
    // Create games store
    if (!db.objectStoreNames.contains(STORES.GAMES)) {
      const gamesStore = db.createObjectStore(STORES.GAMES, { keyPath: 'id' });
      gamesStore.createIndex('title', 'title', { unique: false });
      gamesStore.createIndex('file_path', 'file_path', { unique: true });
      gamesStore.createIndex('crc32', 'crc32', { unique: false });
      gamesStore.createIndex('favorite', 'favorite', { unique: false });
      gamesStore.createIndex('last_played', 'last_played', { unique: false });
    }

    // Create settings store
    if (!db.objectStoreNames.contains(STORES.SETTINGS)) {
      db.createObjectStore(STORES.SETTINGS, { keyPath: 'id' });
    }

    // Create save states store
    if (!db.objectStoreNames.contains(STORES.SAVE_STATES)) {
      const saveStatesStore = db.createObjectStore(STORES.SAVE_STATES, { keyPath: 'id' });
      saveStatesStore.createIndex('gameId', 'game_id', { unique: false });
      saveStatesStore.createIndex('slot', 'slot', { unique: false });
    }

    // Create controller profiles store
    if (!db.objectStoreNames.contains(STORES.CONTROLLER_PROFILES)) {
      const profilesStore = db.createObjectStore(STORES.CONTROLLER_PROFILES, { keyPath: 'id' });
      profilesStore.createIndex('name', 'name', { unique: true });
      profilesStore.createIndex('is_default', 'is_default', { unique: false });
    }

    // Create game metadata store
    if (!db.objectStoreNames.contains(STORES.GAME_METADATA)) {
      const metadataStore = db.createObjectStore(STORES.GAME_METADATA, { keyPath: 'game_id' });
      metadataStore.createIndex('source', 'source', { unique: false });
    }
  },
};

// ============================================================================
// Hook Implementation
// ============================================================================

/**
 * useDatabase - Database hook for local storage operations
 * 
 * @param config - Configuration options for the hook
 * @returns Object containing database state and CRUD operations
 * 
 * @example
 * ```tsx
 * const { 
 *   isReady,
 *   error,
 *   // Game operations
 *   getAllGames,
 *   getGame,
 *   saveGame,
 *   deleteGame,
 *   // Settings operations
 *   getSettings,
 *   saveSettings,
 *   // Save state operations
 *   getSaveStates,
 *   saveSaveState,
 *   deleteSaveState,
 *   // Controller profile operations
 *   getControllerProfiles,
 *   saveControllerProfile,
 *   deleteControllerProfile
 * } = useDatabase({ name: 'oxidesfc-db', version: 1 });
 * ```
 */
export function useDatabase(config: DatabaseConfig = {}) {
  const {
    name = DB_NAME,
    version = DB_VERSION,
    debug = false,
  } = config;

  // State
  const [state, setState] = useState<DatabaseState>({
    isReady: false,
    isLoading: true,
    error: null,
  });

  // Refs
  const dbRef = useRef<IDBDatabase | null>(null);

  // Log helper
  const log = useCallback((...args: unknown[]) => {
    if (debug) {
      console.log('[Database]', ...args);
    }
  }, [debug]);

  // Open database
  const openDatabase = useCallback((): Promise<IDBDatabase> => {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(name, version);

      request.onerror = () => {
        const error = new DatabaseError(
          'Failed to open database',
          'DB_OPEN_ERROR',
          request.error ?? undefined
        );
        reject(error);
      };

      request.onsuccess = () => {
        log('Database opened successfully');
        resolve(request.result);
      };

      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result;
        log('Database upgrade needed', event.oldVersion);

        // Run migrations
        for (let i = event.oldVersion + 1; i <= version; i++) {
          if (migrations[i]) {
            log(`Running migration ${i}`);
            migrations[i](db);
          }
        }
      };
    });
  }, [name, version, log]);

  // Initialize database
  useEffect(() => {
    let isMounted = true;

    const init = async () => {
      try {
        setState(prev => ({ ...prev, isLoading: true, error: null }));
        
        const db = await openDatabase();
        
        if (isMounted) {
          dbRef.current = db;
          
          // Handle database close
          db.onclose = () => {
            log('Database closed');
            dbRef.current = null;
            setState(prev => ({ ...prev, isReady: false }));
          };

          // Handle version change
          db.onversionchange = () => {
            log('Database version change');
            db.close();
          };

          setState(prev => ({
            ...prev,
            isReady: true,
            isLoading: false,
          }));
        }
      } catch (error) {
        if (isMounted) {
          const err = error instanceof Error 
            ? error 
            : new DatabaseError('Unknown error occurred');
          
          setState(prev => ({
            ...prev,
            isLoading: false,
            error: err,
          }));
        }
      }
    };

    init();

    return () => {
      isMounted = false;
      if (dbRef.current) {
        dbRef.current.close();
        dbRef.current = null;
      }
    };
  }, [openDatabase, log]);

  // Generic CRUD operations
  const getAll = useCallback(async <T>(storeName: string): Promise<T[]> => {
    if (!dbRef.current) {
      throw new DatabaseError('Database not ready', 'DB_NOT_READY');
    }

    return new Promise((resolve, reject) => {
      try {
        const transaction = dbRef.current!.transaction(storeName, 'readonly');
        const store = transaction.objectStore(storeName);
        const request = store.getAll();

        request.onsuccess = () => {
          log('GetAll from', storeName, request.result.length, 'items');
          resolve(request.result as T[]);
        };

        request.onerror = () => {
          reject(new DatabaseError(
            `Failed to get all from ${storeName}`,
            'GET_ALL_ERROR',
            request.error ?? undefined
          ));
        };
      } catch (error) {
        reject(new DatabaseError(
          `Error getting all from ${storeName}`,
          'TRANSACTION_ERROR',
          error instanceof Error ? error : undefined
        ));
      }
    });
  }, [log]);

  const get = useCallback(async <T>(storeName: string, key: string): Promise<T | null> => {
    if (!dbRef.current) {
      throw new DatabaseError('Database not ready', 'DB_NOT_READY');
    }

    return new Promise((resolve, reject) => {
      try {
        const transaction = dbRef.current!.transaction(storeName, 'readonly');
        const store = transaction.objectStore(storeName);
        const request = store.get(key);

        request.onsuccess = () => {
          log('Get from', storeName, key, request.result ? 'found' : 'not found');
          resolve(request.result as T ?? null);
        };

        request.onerror = () => {
          reject(new DatabaseError(
            `Failed to get from ${storeName}`,
            'GET_ERROR',
            request.error ?? undefined
          ));
        };
      } catch (error) {
        reject(new DatabaseError(
          `Error getting from ${storeName}`,
          'TRANSACTION_ERROR',
          error instanceof Error ? error : undefined
        ));
      }
    });
  }, [log]);

  const put = useCallback(async <T>(storeName: string, value: T): Promise<void> => {
    if (!dbRef.current) {
      throw new DatabaseError('Database not ready', 'DB_NOT_READY');
    }

    return new Promise((resolve, reject) => {
      try {
        const transaction = dbRef.current!.transaction(storeName, 'readwrite');
        const store = transaction.objectStore(storeName);
        const request = store.put(value);

        request.onsuccess = () => {
          log('Put to', storeName, 'success');
          resolve();
        };

        request.onerror = () => {
          reject(new DatabaseError(
            `Failed to put to ${storeName}`,
            'PUT_ERROR',
            request.error ?? undefined
          ));
        };
      } catch (error) {
        reject(new DatabaseError(
          `Error putting to ${storeName}`,
          'TRANSACTION_ERROR',
          error instanceof Error ? error : undefined
        ));
      }
    });
  }, [log]);

  const remove = useCallback(async (storeName: string, key: string): Promise<void> => {
    if (!dbRef.current) {
      throw new DatabaseError('Database not ready', 'DB_NOT_READY');
    }

    return new Promise((resolve, reject) => {
      try {
        const transaction = dbRef.current!.transaction(storeName, 'readwrite');
        const store = transaction.objectStore(storeName);
        const request = store.delete(key);

        request.onsuccess = () => {
          log('Delete from', storeName, key, 'success');
          resolve();
        };

        request.onerror = () => {
          reject(new DatabaseError(
            `Failed to delete from ${storeName}`,
            'DELETE_ERROR',
            request.error ?? undefined
          ));
        };
      } catch (error) {
        reject(new DatabaseError(
          `Error deleting from ${storeName}`,
          'TRANSACTION_ERROR',
          error instanceof Error ? error : undefined
        ));
      }
    });
  }, [log]);

  const clear = useCallback(async (storeName: string): Promise<void> => {
    if (!dbRef.current) {
      throw new DatabaseError('Database not ready', 'DB_NOT_READY');
    }

    return new Promise((resolve, reject) => {
      try {
        const transaction = dbRef.current!.transaction(storeName, 'readwrite');
        const store = transaction.objectStore(storeName);
        const request = store.clear();

        request.onsuccess = () => {
          log('Clear', storeName, 'success');
          resolve();
        };

        request.onerror = () => {
          reject(new DatabaseError(
            `Failed to clear ${storeName}`,
            'CLEAR_ERROR',
            request.error ?? undefined
          ));
        };
      } catch (error) {
        reject(new DatabaseError(
          `Error clearing ${storeName}`,
          'TRANSACTION_ERROR',
          error instanceof Error ? error : undefined
        ));
      }
    });
  }, [log]);

  // ==========================================================================
  // Game Operations
  // ==========================================================================

  const getAllGames = useCallback(async (): Promise<Game[]> => {
    return getAll<Game>(STORES.GAMES);
  }, [getAll]);

  const getGame = useCallback(async (id: string): Promise<Game | null> => {
    return get<Game>(STORES.GAMES, id);
  }, [get]);

  const getGameByPath = useCallback(async (filePath: string): Promise<Game | null> => {
    const games = await getAllGames();
    return games.find(g => g.file_path === filePath) ?? null;
  }, [getAllGames]);

  const saveGame = useCallback(async (game: Game): Promise<void> => {
    return put(STORES.GAMES, game);
  }, [put]);

  const deleteGame = useCallback(async (id: string): Promise<void> => {
    return remove(STORES.GAMES, id);
  }, [remove]);

  const updateGamePlayCount = useCallback(async (id: string): Promise<void> => {
    const game = await getGame(id);
    if (game) {
      game.play_count += 1;
      game.last_played = new Date().toISOString();
      await saveGame(game);
    }
  }, [getGame, saveGame]);

  const toggleGameFavorite = useCallback(async (id: string): Promise<void> => {
    const game = await getGame(id);
    if (game) {
      game.favorite = !game.favorite;
      await saveGame(game);
    }
  }, [getGame, saveGame]);

  // ==========================================================================
  // Settings Operations
  // ==========================================================================

  const getSettings = useCallback(async (): Promise<AppSettings | null> => {
    return get<AppSettings>(STORES.SETTINGS, 'app-settings');
  }, [get]);

  const saveSettings = useCallback(async (settings: AppSettings): Promise<void> => {
    return put(STORES.SETTINGS, { ...settings, id: 'app-settings' });
  }, [put]);

  // ==========================================================================
  // Save State Operations
  // ==========================================================================

  interface SaveStateData {
    id: string;
    game_id: string;
    slot: number;
    data: ArrayBuffer;
    screenshot: string | null;
    timestamp: number;
  }

  const getSaveStates = useCallback(async (gameId?: string): Promise<SaveStateData[]> => {
    const states = await getAll<SaveStateData>(STORES.SAVE_STATES);
    if (gameId) {
      return states.filter(s => s.game_id === gameId);
    }
    return states;
  }, [getAll]);

  const getSaveState = useCallback(async (gameId: string, slot: number): Promise<SaveStateData | null> => {
    const states = await getSaveStates(gameId);
    return states.find(s => s.slot === slot) ?? null;
  }, [getSaveStates]);

  const saveSaveState = useCallback(async (state: SaveStateData): Promise<void> => {
    return put(STORES.SAVE_STATES, state);
  }, [put]);

  const deleteSaveState = useCallback(async (id: string): Promise<void> => {
    return remove(STORES.SAVE_STATES, id);
  }, [remove]);

  // ==========================================================================
  // Controller Profile Operations
  // ==========================================================================

  const getControllerProfiles = useCallback(async (): Promise<ControllerProfile[]> => {
    return getAll<ControllerProfile>(STORES.CONTROLLER_PROFILES);
  }, [getAll]);

  const getControllerProfile = useCallback(async (id: string): Promise<ControllerProfile | null> => {
    return get<ControllerProfile>(STORES.CONTROLLER_PROFILES, id);
  }, [get]);

  const getDefaultControllerProfile = useCallback(async (): Promise<ControllerProfile | null> => {
    const profiles = await getControllerProfiles();
    return profiles.find(p => p.is_default) ?? null;
  }, [getControllerProfiles]);

  const saveControllerProfile = useCallback(async (profile: ControllerProfile): Promise<void> => {
    // If this is the default, unset other defaults
    if (profile.is_default) {
      const profiles = await getControllerProfiles();
      for (const p of profiles) {
        if (p.is_default && p.id !== profile.id) {
          await put(STORES.CONTROLLER_PROFILES, { ...p, is_default: false });
        }
      }
    }
    return put(STORES.CONTROLLER_PROFILES, profile);
  }, [getControllerProfiles, put]);

  const deleteControllerProfile = useCallback(async (id: string): Promise<void> => {
    return remove(STORES.CONTROLLER_PROFILES, id);
  }, [remove]);

  // ==========================================================================
  // Game Metadata Operations
  // ==========================================================================

  interface GameMetadata {
    game_id: string;
    data: Record<string, unknown>;
    source: string;
    fetched_at: string;
  }

  const getGameMetadata = useCallback(async (gameId: string): Promise<GameMetadata | null> => {
    return get<GameMetadata>(STORES.GAME_METADATA, gameId);
  }, [get]);

  const saveGameMetadata = useCallback(async (metadata: GameMetadata): Promise<void> => {
    return put(STORES.GAME_METADATA, metadata);
  }, [put]);

  const deleteGameMetadata = useCallback(async (gameId: string): Promise<void> => {
    return remove(STORES.GAME_METADATA, gameId);
  }, [remove]);

  // ==========================================================================
  // Utility Operations
  // ==========================================================================

  const clearAllData = useCallback(async (): Promise<void> => {
    await clear(STORES.GAMES);
    await clear(STORES.SETTINGS);
    await clear(STORES.SAVE_STATES);
    await clear(STORES.CONTROLLER_PROFILES);
    await clear(STORES.GAME_METADATA);
  }, [clear]);

  const getStorageUsage = useCallback(async (): Promise<{ used: number; quota: number }> => {
    if (navigator.storage && navigator.storage.estimate) {
      const estimate = await navigator.storage.estimate();
      return {
        used: estimate.usage ?? 0,
        quota: estimate.quota ?? 0,
      };
    }
    return { used: 0, quota: 0 };
  }, []);

  return {
    // State
    ...state,

    // Game operations
    getAllGames,
    getGame,
    getGameByPath,
    saveGame,
    deleteGame,
    updateGamePlayCount,
    toggleGameFavorite,

    // Settings operations
    getSettings,
    saveSettings,

    // Save state operations
    getSaveStates,
    getSaveState,
    saveSaveState,
    deleteSaveState,

    // Controller profile operations
    getControllerProfiles,
    getControllerProfile,
    getDefaultControllerProfile,
    saveControllerProfile,
    deleteControllerProfile,

    // Game metadata operations
    getGameMetadata,
    saveGameMetadata,
    deleteGameMetadata,

    // Utility operations
    clearAllData,
    getStorageUsage,
  };
}

// ============================================================================
// Type Exports
// ============================================================================
// Note: Types are exported inline with the interfaces
