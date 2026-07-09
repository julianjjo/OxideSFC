/**
 * Cheat Types
 * 
 * Type definitions for the cheat code manager.
 */



// ============================================================================
// Cheat Code Format
// ============================================================================

export type CheatCodeFormat = 'gamegenie' | 'proactionreplay' | 'goldfinger' | 'raw';

// ============================================================================
// Cheat Code Type
// ============================================================================

export interface CheatCode {
  /** Unique identifier */
  id: string;
  /** Cheat name */
  name: string;
  /** Cheat description */
  description: string;
  /** The actual code */
  code: string;
  /** Code format */
  format: CheatCodeFormat;
  /** Whether the cheat is enabled */
  enabled: boolean;
  /** Game ID this cheat belongs to */
  gameId: string;
  /** Created timestamp */
  createdAt: string;
  /** Updated timestamp */
  updatedAt: string;
}

// ============================================================================
// Cheat Category
// ============================================================================

export type CheatCategory = 
  | 'unlimited_lives'
  | 'unlimited_health'
  | 'unlimited_power'
  | 'unlock_content'
  | 'level_select'
  | 'invincibility'
  | 'infinite_items'
  | 'speed'
  | 'other';

// ============================================================================
// Cheat Database Entry
// ============================================================================

export interface CheatDatabaseEntry {
  /** Game identifier (CRC32 or internal name) */
  gameId: string;
  /** Game title */
  gameTitle: string;
  /** Cheat codes */
  cheats: CheatCode[];
}

// ============================================================================
// Cheat Database
// ============================================================================

export interface CheatDatabase {
  /** Database version */
  version: number;
  /** Database name */
  name: string;
  /** Last updated */
  updatedAt: string;
  /** Entries */
  entries: CheatDatabaseEntry[];
}

// ============================================================================
// Common SNES Cheat Code Formats
// ============================================================================

/**
 * Parse a Game Genie code
 * Game Genie codes are 6 or 9 characters
 */
export function parseGameGenieCode(code: string): string | null {
  const cleaned = code.replace(/[^A-Z0-9]/gi, '').toUpperCase();
  if (cleaned.length !== 6 && cleaned.length !== 9) return null;
  return cleaned;
}

/**
 * Parse a Pro Action Replay code
 * Pro Action Replay codes are 8 characters
 */
export function parseProActionReplayCode(code: string): string | null {
  const cleaned = code.replace(/[^A-F0-9]/gi, '').toUpperCase();
  if (cleaned.length !== 8) return null;
  return cleaned;
}

/**
 * Parse a Gold Finger code
 * Gold Finger codes are variable length
 */
export function parseGoldFingerCode(code: string): string | null {
  const cleaned = code.replace(/[^A-Z0-9]/gi, '').toUpperCase();
  if (cleaned.length < 4) return null;
  return cleaned;
}

/**
 * Detect code format automatically
 */
export function detectCodeFormat(code: string): CheatCodeFormat {
  const cleaned = code.replace(/[^A-Z0-9]/gi, '').toUpperCase();
  
  // Game Genie: 6 or 9 chars, mix of letters and numbers
  if ((cleaned.length === 6 || cleaned.length === 9) && /^[A-Z0-9]+$/.test(cleaned)) {
    return 'gamegenie';
  }
  
  // Pro Action Replay: 8 hex chars
  if (cleaned.length === 8 && /^[A-F0-9]+$/.test(cleaned)) {
    return 'proactionreplay';
  }
  
  // Gold Finger: starts with specific patterns
  if (cleaned.startsWith('GF') || cleaned.startsWith('F0')) {
    return 'goldfinger';
  }
  
  // Default to raw if can't determine
  return 'raw';
}

/**
 * Validate a cheat code
 */
export function validateCheatCode(code: string, format?: CheatCodeFormat): boolean {
  const detectedFormat = format || detectCodeFormat(code);
  
  switch (detectedFormat) {
    case 'gamegenie':
      return parseGameGenieCode(code) !== null;
    case 'proactionreplay':
      return parseProActionReplayCode(code) !== null;
    case 'goldfinger':
      return parseGoldFingerCode(code) !== null;
    default:
      return code.trim().length > 0;
  }
}

// ============================================================================
// SNES Button Combinations (for turbo)
// ============================================================================

export const TURBO_BUTTONS = [
  { id: 'turbo_a', label: 'Turbo A', key: 'a' },
  { id: 'turbo_b', label: 'Turbo B', key: 'b' },
  { id: 'turbo_x', label: 'Turbo X', key: 'x' },
  { id: 'turbo_y', label: 'Turbo Y', key: 'y' },
  { id: 'turbo_l', label: 'Turbo L', key: 'l' },
  { id: 'turbo_r', label: 'Turbo R', key: 'r' },
] as const;

// ============================================================================
// Category Labels
// ============================================================================

export const CHEAT_CATEGORY_LABELS: Record<CheatCategory, string> = {
  unlimited_lives: 'Unlimited Lives',
  unlimited_health: 'Unlimited Health',
  unlimited_power: 'Unlimited Power',
  unlock_content: 'Unlock Content',
  level_select: 'Level Select',
  invincibility: 'Invincibility',
  infinite_items: 'Infinite Items',
  speed: 'Speed',
  other: 'Other',
};
