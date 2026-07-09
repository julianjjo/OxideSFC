/**
 * Cheat Manager Component
 * 
 * Exports the CheatManager component and types.
 */

export { CheatManager } from './CheatManager';
export type { CheatManagerProps } from './CheatManager';
export type {
  CheatCode,
  CheatCodeFormat,
  CheatCategory,
  CheatDatabase,
  CheatDatabaseEntry,
} from './types';
export {
  validateCheatCode,
  detectCodeFormat,
  CHEAT_CATEGORY_LABELS,
  TURBO_BUTTONS,
} from './types';
