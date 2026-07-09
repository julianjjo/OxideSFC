/**
 * Domain Types for OxideSFC Frontend
 * 
 * This module contains all domain entities and types used throughout
 * the application, providing a central location for type definitions.
 */

// ============================================================================
// ROM Types
// ============================================================================

/**
 * ROM file format types
 */
export enum RomFormat {
  BARE = 'bare',
  SMC = 'smc',
  FIG = 'fig',
  SWC = 'swc',
  ZIP = 'zip',
  SEVEN_ZIP = '7z',
  RAR = 'rar',
}

/**
 * ROM memory mapping types
 */
export enum MemoryMapping {
  LOROM = 'lorom',
  HIROM = 'hirom',
  EXHIROM = 'exhirom',
  UNKNOWN = 'unknown',
}

/**
 * ROM region types
 */
export enum RomRegion {
  DOMESTIC = 'domestic',   // Japan
  EXPORT = 'export',       // USA/Europe
  INTERNATIONAL = 'international',
  UNKNOWN = 'unknown',
}

/**
 * Country codes for ROMs
 */
export enum RomCountry {
  USA = 'USA',
  EUROPE = 'Europe',
  JAPAN = 'Japan',
  KOREA = 'Korea',
  BRAZIL = 'Brazil',
  CHINA = 'China',
  UNKNOWN = 'unknown',
}

// ============================================================================
// Game Entity
// ============================================================================

/**
 * Game entity representing a ROM file in the library
 */
export interface Game {
  id: string;
  title: string;
  file_path: string;
  file_name: string;
  file_size: number;
  crc32: string | null;
  md5: string | null;
  sha256: string | null;
  
  // ROM information
  rom_type: string;
  rom_format: RomFormat;
  memory_mapping: MemoryMapping;
  rom_size: number;
  sram_size: number;
  country: RomCountry;
  region: RomRegion;
  
  // Metadata
  description: string | null;
  release_date: string | null;
  developer: string | null;
  publisher: string | null;
  genre: string | null;
  players: number;
  rating: number | null;
  
  // Frontend-specific
  play_count: number;
  last_played: string | null;
  favorite: boolean;
  custom_cover_path: string | null;
  
  // Status
  is_valid: boolean;
  validation_errors: string[];
  
  // Timestamps
  created_at: string;
  updated_at: string;
}

/**
 * Folder/Collection for organizing games
 */
export interface GameFolder {
  id: string;
  name: string;
  parent_id: string | null;
  created_at: string;
}

/**
 * Game-Folder relationship
 */
export interface GameFolderRelation {
  game_id: string;
  folder_id: string;
}

// ============================================================================
// Settings Interfaces
// ============================================================================

/**
 * Video settings for emulation
 */
export interface VideoSettings {
  vsync: boolean;
  frame_limit: string;
  renderer: 'webgl' | 'webgpu' | 'canvas';
  shader: string;
  scale_mode: 'nearest' | 'bilinear' | 'xbrz' | 'hq2x';
  aspect_ratio: 'original' | '4:3' | '16:9';
  crt_mode: boolean;
}

/**
 * Audio settings for emulation
 */
export interface AudioSettings {
  enabled: boolean;
  volume: number;
  latency: number;
  sample_rate: number;
  channels: 'stereo' | 'mono';
}

/**
 * Control settings
 */
export interface ControlSettings {
  keyboard_enabled: boolean;
  gamepad_enabled: boolean;
  keyboard_mapping: KeyboardMapping;
  gamepad_profile: string;
}

/**
 * Keyboard key to button mapping
 */
export interface KeyboardMapping {
  [key: string]: InputButton;
}

/**
 * Input button identifiers
 */
export type InputButton = 
  | 'up' 
  | 'down' 
  | 'left' 
  | 'right' 
  | 'a' 
  | 'b' 
  | 'x' 
  | 'y'
  | 'start' 
  | 'select'
  | 'l' 
  | 'r'
  | 'l_analog'
  | 'r_analog';

/**
 * Gamepad profile configuration
 */
export interface GamepadProfile {
  id: string;
  name: string;
  is_default: boolean;
  button_mapping: GamepadButtonMapping;
  deadzone: number;
}

/**
 * Gamepad button mapping
 */
export interface GamepadButtonMapping {
  a: number;
  b: number;
  x: number;
  y: number;
  start: number;
  select: number;
  l: number;
  r: number;
  dpad_up: number;
  dpad_down: number;
  dpad_left: number;
  dpad_right: number;
  l_analog_x: number;
  l_analog_y: number;
  r_analog_x: number;
  r_analog_y: number;
}

/**
 * Library settings
 */
export interface LibrarySettings {
  folders: string[];
  scan_recursive: boolean;
  use_metadata: boolean;
  cover_resolution: 'thumbnail' | 'small' | 'medium' | 'large';
  skip_known_games: boolean;
  verify_hashes: boolean;
}

/**
 * General application settings
 */
export interface GeneralSettings {
  language: string;
  theme: 'dark' | 'light' | 'system';
  show_window_on_start: boolean;
  confirm_on_exit: boolean;
  check_updates: boolean;
}

/**
 * Complete application settings
 */
export interface AppSettings {
  general: GeneralSettings;
  video: VideoSettings;
  audio: AudioSettings;
  controls: ControlSettings;
  library: LibrarySettings;
}

// ============================================================================
// Emulation Types
// ============================================================================

/**
 * Video frame data from emulation
 */
export interface VideoFrame {
  width: number;
  height: number;
  data: Uint8Array | number[];
  timestamp: number;
}

/**
 * Input state for controllers
 */
export interface InputState {
  buttons: number;
  x: number;
  y: number;
}

/**
 * Gamepad state for connected controllers
 */
export interface GamepadState {
  index: number;
  id: string;
  connected: boolean;
  buttons: GamepadButtonState[];
  axes: number[];
  timestamp: number;
}

/**
 * Individual gamepad button state
 */
export interface GamepadButtonState {
  index: number;
  pressed: boolean;
  value: number;
}

/**
 * Game information loaded from ROM
 */
export interface GameInfo {
  id: string;
  title: string;
  file_path: string;
  file_size: number;
  rom_type: string;
  rom_format: RomFormat;
  memory_mapping: MemoryMapping;
  rom_size: number;
  sram_size: number;
  region: RomRegion;
  country: RomCountry;
  is_valid: boolean;
  validation_errors: string[];
}

// ============================================================================
// Scan/Import Types
// ============================================================================

/**
 * Scan configuration for library scanning
 */
export interface ScanConfig {
  directories: string[];
  recursive: boolean;
  skipHidden: boolean;
  extensions: string[];
  verifyHashes: boolean;
  extractArchives: boolean;
}

/**
 * Scan progress information
 */
export interface ScanProgress {
  total: number;
  current: number;
  currentFile: string;
  errors: string[];
}

/**
 * Scan result
 */
export interface ScanResult {
  games: Game[];
  total: number;
  errors: string[];
}

// ============================================================================
// Controller Profile Types
// ============================================================================

/**
 * Controller profile stored in database
 */
export interface ControllerProfile {
  id: string;
  name: string;
  is_default: boolean;
  config: GamepadProfile;
  created_at: string;
  updated_at: string;
}

// ============================================================================
// Metadata Types
// ============================================================================

/**
 * Game metadata from external sources
 */
export interface GameMetadata {
  game_id: string;
  title: string;
  alternate_titles: string[];
  description: string;
  release_date: string | null;
  developer: string | null;
  publisher: string | null;
  genre: string | null;
  players: number;
  rating: number | null;
  cover_url: string | null;
  /**
   * 3D box art URL, when the source provides one (e.g. Screenscraper's
   * `box3d` media). Distinct from `cover_url`, which is the 2D box art.
   */
  cover_url_3d?: string | null;
  source: 'local' | 'screenscraper' | 'igdb' | 'openvgdb';
}

// ============================================================================
// Command/History Types
// ============================================================================

/**
 * Command for undo/redo functionality
 */
export interface Command {
  id: string;
  type: string;
  execute: () => Promise<void> | void;
  undo: () => Promise<void> | void;
  description: string;
  timestamp: number;
}

/**
 * Command history entry
 */
export interface CommandHistoryEntry {
  command: Command;
  executedAt: number;
}

// ============================================================================
// Event Types
// ============================================================================

/**
 * Emulation event payloads
 */
export interface EmulationEvents {
  'emulation:start': { game: GameInfo };
  'emulation:pause': void;
  'emulation:resume': void;
  'emulation:stop': void;
  'emulation:frame': { frame: VideoFrame };
}

/**
 * Library event payloads
 */
export interface LibraryEvents {
  'library:scan:start': { directories: string[] };
  'library:scan:progress': { progress: ScanProgress };
  'library:scan:complete': { result: ScanResult };
}

/**
 * Settings event payloads
 */
export interface SettingsEvents {
  'settings:change': { key: string; value: unknown; previousValue: unknown };
}

/**
 * Input event payloads
 */
export interface InputEvents {
  'input:button': { button: InputButton; pressed: boolean };
  'input:gamepad:connect': { gamepad: GamepadState };
  'input:gamepad:disconnect': { index: number };
}

// ============================================================================
// Default Values
// ============================================================================

/**
 * Default video settings
 */
export const DEFAULT_VIDEO_SETTINGS: VideoSettings = {
  vsync: true,
  frame_limit: '60',
  renderer: 'webgl',
  shader: 'none',
  scale_mode: 'bilinear',
  aspect_ratio: 'original',
  crt_mode: false,
};

/**
 * Default audio settings
 */
export const DEFAULT_AUDIO_SETTINGS: AudioSettings = {
  enabled: true,
  volume: 1.0,
  latency: 50,
  sample_rate: 44100,
  channels: 'stereo',
};

/**
 * Default control settings
 */
export const DEFAULT_CONTROL_SETTINGS: ControlSettings = {
  keyboard_enabled: true,
  gamepad_enabled: true,
  keyboard_mapping: {
    'ArrowUp': 'up',
    'ArrowDown': 'down',
    'ArrowLeft': 'left',
    'ArrowRight': 'right',
    'KeyZ': 'a',
    'KeyX': 'b',
    'Enter': 'start',
    'ShiftRight': 'select',
    'KeyA': 'l',
    'KeyS': 'r',
  },
  gamepad_profile: 'default',
};

/**
 * Default library settings
 */
export const DEFAULT_LIBRARY_SETTINGS: LibrarySettings = {
  folders: [],
  scan_recursive: true,
  use_metadata: true,
  cover_resolution: 'medium',
  skip_known_games: true,
  verify_hashes: true,
};

/**
 * Default general settings
 */
export const DEFAULT_GENERAL_SETTINGS: GeneralSettings = {
  language: 'en',
  theme: 'dark',
  show_window_on_start: true,
  confirm_on_exit: true,
  check_updates: true,
};

/**
 * Default app settings
 */
export const DEFAULT_APP_SETTINGS: AppSettings = {
  general: DEFAULT_GENERAL_SETTINGS,
  video: DEFAULT_VIDEO_SETTINGS,
  audio: DEFAULT_AUDIO_SETTINGS,
  controls: DEFAULT_CONTROL_SETTINGS,
  library: DEFAULT_LIBRARY_SETTINGS,
};
