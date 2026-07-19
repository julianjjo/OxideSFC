/**
 * File System Service
 * 
 * Provides file system operations through Tauri, including:
 * - File dialogs (open/save)
 * - ROM reading
 * - Save file management
 * - App data path management
 */

import { open, save } from '@tauri-apps/plugin-dialog';
import { readFile, writeFile, mkdir, exists } from '@tauri-apps/plugin-fs';
import { appDataDir, join } from '@tauri-apps/api/path';

// ============================================================================
// File System Types
// ============================================================================

/**
 * File filter for dialogs
 */
export interface FileFilter {
  name: string;
  extensions: string[];
}

/**
 * Open file dialog options
 */
export interface OpenDialogOptions {
  title?: string;
  defaultPath?: string;
  filters?: FileFilter[];
  multiple?: boolean;
  directory?: boolean;
}

/**
 * Save file dialog options
 */
export interface SaveDialogOptions {
  title?: string;
  defaultPath?: string;
  filters?: FileFilter[];
}

/**
 * ROM file information
 */
export interface RomFileInfo {
  path: string;
  name: string;
  size: number;
  extension: string;
}

/**
 * Save file information
 */
export interface SaveFileInfo {
  path: string;
  gameId: string;
  slot: number;
  timestamp: number;
  size: number;
}

// ============================================================================
// Default Filters
// ============================================================================

/**
 * Default ROM file filters
 */
export const ROM_FILTERS: FileFilter[] = [
  { name: 'SNES ROMs', extensions: ['sfc', 'smc', 'fig', 'swc', 'zip'] },
  { name: 'All Files', extensions: ['*'] },
];

/**
 * Save file filters
 */
export const SAVE_FILTERS: FileFilter[] = [
  { name: 'Save Files', extensions: ['sav', 'srm'] },
  { name: 'All Files', extensions: ['*'] },
];

// ============================================================================
// File System Service Implementation
// ============================================================================

/**
 * File System Service
 * 
 * Provides cross-platform file system operations through Tauri.
 */
class FileSystemServiceImpl {
  private appDataPath: string | null = null;

  // ==========================================================================
  // Dialog Methods
  // ==========================================================================

  /**
   * Open a file dialog
   * @param options - Dialog options
   * @returns Selected file path(s) or null if cancelled
   */
  async openFile(options: OpenDialogOptions = {}): Promise<string | string[] | null> {
    const result = await open({
      title: options.title ?? 'Open File',
      defaultPath: options.defaultPath,
      filters: options.filters ?? ROM_FILTERS,
      multiple: options.multiple ?? false,
      directory: options.directory ?? false,
    });

    return result;
  }

  /**
   * Open a ROM file
   * @param options - Dialog options
   * @returns Selected ROM file path or null if cancelled
   */
  async openRom(options: Partial<OpenDialogOptions> = {}): Promise<string | null> {
    const result = await open({
      title: options.title ?? 'Open ROM',
      defaultPath: options.defaultPath,
      filters: options.filters ?? ROM_FILTERS,
      multiple: false,
      directory: false,
    });

    return result as string | null;
  }

  /**
   * Open multiple ROM files
   * @param options - Dialog options
   * @returns Selected ROM file paths or empty array if cancelled
   */
  async openMultipleRoms(options: Partial<OpenDialogOptions> = {}): Promise<string[]> {
    const result = await open({
      title: options.title ?? 'Open ROMs',
      defaultPath: options.defaultPath,
      filters: options.filters ?? ROM_FILTERS,
      multiple: true,
      directory: false,
    });

    if (Array.isArray(result)) {
      return result;
    } else if (result) {
      return [result];
    }
    return [];
  }

  /**
   * Open a folder dialog
   * @param options - Dialog options
   * @returns Selected folder path or null if cancelled
   */
  async openFolder(options: Partial<OpenDialogOptions> = {}): Promise<string | null> {
    const result = await open({
      title: options.title ?? 'Select Folder',
      defaultPath: options.defaultPath,
      directory: true,
      multiple: false,
    });

    return result as string | null;
  }

  /**
   * Open a save file dialog
   * @param options - Dialog options
   * @returns Selected save file path or null if cancelled
   */
  async saveFile(options: SaveDialogOptions = {}): Promise<string | null> {
    const result = await save({
      title: options.title ?? 'Save File',
      defaultPath: options.defaultPath,
      filters: options.filters ?? SAVE_FILTERS,
    });

    return result;
  }

  // ==========================================================================
  // ROM Reading Methods
  // ==========================================================================

  /**
   * Read a ROM file
   * @param path - Path to the ROM file
   * @returns Promise resolving to ROM data as Uint8Array
   */
  async readRom(path: string): Promise<Uint8Array> {
    const data = await readFile(path);
    return data;
  }

  /**
   * Read a ROM file as ArrayBuffer
   * @param path - Path to the ROM file
   * @returns Promise resolving to ROM data as ArrayBuffer
   */
  async readRomAsArrayBuffer(path: string): Promise<ArrayBuffer> {
    const data = await this.readRom(path);
    // Create a new ArrayBuffer and copy the data
    const arrayBuffer = new ArrayBuffer(data.length);
    new Uint8Array(arrayBuffer).set(data);
    return arrayBuffer;
  }

  /**
   * Get ROM file information
   * @param path - Path to the ROM file
   * @returns Promise resolving to ROM file info
   */
  async getRomInfo(path: string): Promise<RomFileInfo> {
    const data = await this.readRom(path);
    const name = path.split(/[\\/]/).pop() ?? 'unknown';
    const extension = name.split('.').pop()?.toLowerCase() ?? '';

    return {
      path,
      name,
      size: data.length,
      extension,
    };
  }

  // ==========================================================================
  // Save File Methods
  // ==========================================================================

  /**
   * Write a save file
   * @param path - Path to save the file
   * @param data - Data to save
   */
  async writeSave(path: string, data: Uint8Array): Promise<void> {
    await writeFile(path, data);
  }

  /**
   * Read a save file
   * @param path - Path to the save file
   * @returns Promise resolving to save data
   */
  async readSave(path: string): Promise<Uint8Array> {
    const data = await readFile(path);
    return data;
  }

  /**
   * Get save file path for a game
   * @param gameId - Game identifier
   * @param slot - Save slot number
   * @returns Full path to the save file
   */
  async getSavePath(gameId: string, slot: number = 0): Promise<string> {
    const savesDir = await this.ensureDirectory('saves');
    const ext = slot === 0 ? 'srm' : `sav${slot}`;
    return join(savesDir, `${gameId}.${ext}`);
  }

  /**
   * Save game state
   * @param gameId - Game identifier
   * @param data - Save data
   * @param slot - Save slot number
   */
  async saveGame(gameId: string, data: Uint8Array, slot: number = 0): Promise<string> {
    const path = await this.getSavePath(gameId, slot);
    await this.writeSave(path, data);
    return path;
  }

  /**
   * Load game state
   * @param gameId - Game identifier
   * @param slot - Save slot number
   * @returns Save data or null if not found
   */
  async loadGame(gameId: string, slot: number = 0): Promise<Uint8Array | null> {
    const path = await this.getSavePath(gameId, slot);
    
    if (await this.exists(path)) {
      return this.readSave(path);
    }
    
    return null;
  }

  /**
   * Check if a save file exists
   * @param gameId - Game identifier
   * @param slot - Save slot number
   * @returns true if save file exists
   */
  async saveExists(gameId: string, slot: number = 0): Promise<boolean> {
    const path = await this.getSavePath(gameId, slot);
    return this.exists(path);
  }

  // ==========================================================================
  // App Data Methods
  // ==========================================================================

  /**
   * Get the application data directory
   * @returns Promise resolving to app data path
   */
  async getAppDataPath(): Promise<string> {
    if (!this.appDataPath) {
      this.appDataPath = await appDataDir();
    }
    return this.appDataPath;
  }

  /**
   * Ensure a directory exists
   * @param path - Directory path relative to app data
   * @returns Full path to the directory
   */
  async ensureDirectory(path: string): Promise<string> {
    const appData = await this.getAppDataPath();
    const fullPath = await join(appData, path);
    
    if (!(await exists(fullPath))) {
      await mkdir(fullPath, { recursive: true });
    }
    
    return fullPath;
  }

  /**
   * Get a path within the app data directory
   * @param path - Path relative to app data
   * @returns Full path
   */
  async getPath(path: string): Promise<string> {
    const appData = await this.getAppDataPath();
    return join(appData, path);
  }

  /**
   * Check if a path exists
   * @param path - Path to check
   * @returns true if path exists
   */
  async exists(path: string): Promise<boolean> {
    return exists(path);
  }

  // ==========================================================================
  // Utility Methods
  // ==========================================================================

  /**
   * Get file extension
   * @param filename - File name
   * @returns File extension without dot
   */
  getExtension(filename: string): string {
    const parts = filename.split('.');
    return parts.length > 1 ? parts.pop()!.toLowerCase() : '';
  }

  /**
   * Get file name without extension
   * @param filename - File name
   * @returns File name without extension
   */
  getBaseName(filename: string): string {
    const parts = filename.split('.');
    if (parts.length > 1) {
      parts.pop();
    }
    return parts.join('.');
  }
}

// ============================================================================
// Singleton Instance
// ============================================================================

let fileSystemServiceInstance: FileSystemServiceImpl | null = null;

/**
 * Get the file system service singleton
 */
export function getFileSystemService(): FileSystemServiceImpl {
  if (!fileSystemServiceInstance) {
    fileSystemServiceInstance = new FileSystemServiceImpl();
  }
  return fileSystemServiceInstance;
}


