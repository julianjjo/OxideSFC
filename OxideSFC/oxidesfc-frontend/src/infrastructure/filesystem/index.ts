/**
 * Filesystem Infrastructure Module
 * 
 * Provides file system operations through Tauri, including:
 * - File dialogs (open/save)
 * - ROM reading
 * - Save file management
 * - App data path management
 */

export { getFileSystemService } from './FileSystemService';
export type { FileFilter, OpenDialogOptions, SaveDialogOptions, RomFileInfo, SaveFileInfo } from './FileSystemService';
export { ROM_FILTERS, SAVE_FILTERS } from './FileSystemService';
