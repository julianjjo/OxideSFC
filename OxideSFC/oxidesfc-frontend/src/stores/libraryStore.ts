import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface Game {
  id: string;
  title: string;
  file_path: string;
  file_name: string;
  file_size: number;
  rom_type: string;
  sram_size: number;
  country: string;
  play_count: number;
  last_played: string | null;
  favorite: boolean;
  custom_cover_path: string | null;
  created_at: string;
  updated_at: string;
}

export interface ScanResult {
  games: Game[];
  total: number;
  errors: string[];
}

interface LibraryState {
  games: Game[];
  isLoading: boolean;
  isScanning: boolean;
  searchQuery: string;
  sortBy: 'title' | 'last_played' | 'play_count' | 'favorite';
  sortOrder: 'asc' | 'desc';
  viewMode: 'grid' | 'list';
  
  // Actions
  loadGames: () => Promise<void>;
  scanDirectory: (path: string, recursive?: boolean) => Promise<ScanResult>;
  removeGame: (gameId: string) => Promise<void>;
  toggleFavorite: (gameId: string) => Promise<boolean>;
  setSearchQuery: (query: string) => void;
  setSortBy: (sortBy: 'title' | 'last_played' | 'play_count' | 'favorite') => void;
  setSortOrder: (order: 'asc' | 'desc') => void;
  setViewMode: (mode: 'grid' | 'list') => void;
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  games: [],
  isLoading: false,
  isScanning: false,
  searchQuery: '',
  sortBy: 'title',
  sortOrder: 'asc',
  viewMode: 'grid',

  loadGames: async () => {
    set({ isLoading: true });
    try {
      const games = await invoke<Game[]>('get_games');
      set({ games, isLoading: false });
    } catch (error) {
      console.error('Failed to load games:', error);
      set({ isLoading: false });
    }
  },

  scanDirectory: async (path: string, _recursive = true) => {
    set({ isScanning: true });
    try {
      // `scan_directory` only scans and returns results in memory -- it never
      // writes to library.json. `add_game_folder` does the same scan and then
      // persists (deduping by file_path), which is what the reload below needs.
      const result = await invoke<ScanResult>('add_game_folder', { path });

      // Reload games after scanning
      await get().loadGames();
      
      set({ isScanning: false });
      return result;
    } catch (error) {
      console.error('Failed to scan directory:', error);
      set({ isScanning: false });
      throw error;
    }
  },

  removeGame: async (gameId: string) => {
    try {
      await invoke('remove_game', { gameId });
      const games = get().games.filter(g => g.id !== gameId);
      set({ games });
    } catch (error) {
      console.error('Failed to remove game:', error);
      throw error;
    }
  },

  toggleFavorite: async (gameId: string) => {
    // Always derive the new value from a fresh backend round-trip rather
    // than a value captured in a component's closure -- toggle_game_favorite
    // flips the persisted value server-side and returns the result, so this
    // is immune to rapid double-toggles racing each other and cancelling
    // out (the bug this action exists to fix).
    const newValue = await invoke<boolean>('toggle_game_favorite', { gameId });
    const games = get().games.map(g =>
      g.id === gameId ? { ...g, favorite: newValue } : g
    );
    set({ games });
    return newValue;
  },

  setSearchQuery: (query: string) => set({ searchQuery: query }),
  setSortBy: (sortBy: 'title' | 'last_played' | 'play_count' | 'favorite') => set({ sortBy }),
  setSortOrder: (order: 'asc' | 'desc') => set({ sortOrder: order }),
  setViewMode: (mode: 'grid' | 'list') => set({ viewMode: mode }),
}));
