/**
 * Metadata Service
 * Handles retrieval of game metadata from external sources
 * 
 * Supported sources:
 * - Screenscraper.fr (most comprehensive SNES data)
 * - Local cache
 */

import { Game } from '../stores/libraryStore';

// Screenscraper API configuration
const SCREENSCRAPER_BASE_URL = 'https://www.screenscraper.fr';
const SCREENSCRAPER_API_VERSION = 'v2';

// Game metadata from external sources
export interface GameMetadata {
  // ROM info
  crc32?: string;
  md5?: string;
  sha256?: string;
  
  // Game info
  title: string;
  alternateTitles?: string[];
  description?: string;
  releaseDate?: string;
  developer?: string;
  publisher?: string;
  genre?: string;
  players?: number;
  rating?: number;
  
  // Media
  coverUrl?: string;
  screenshotUrls?: string[];
  thumbnailUrl?: string;
  videoUrl?: string;
  
  // Region-specific
  region?: string;
  language?: string;
}

// Configuration for metadata lookup
export interface MetadataLookupOptions {
  preferredSource: 'screenscraper' | 'local';
  includeCovers: boolean;
  coverResolution: 'thumbnail' | 'small' | 'medium' | 'large' | 'original';
  forceRefresh?: boolean;
}

// Default options
const DEFAULT_OPTIONS: MetadataLookupOptions = {
  preferredSource: 'screenscraper',
  includeCovers: true,
  coverResolution: 'medium',
  forceRefresh: false,
};

/**
 * Lookup metadata for a game
 * Uses local cache first, then queries external sources
 */
export async function lookupMetadata(
  game: Game,
  options: Partial<MetadataLookupOptions> = {}
): Promise<GameMetadata | null> {
  const opts = { ...DEFAULT_OPTIONS, ...options };
  
  try {
    // Check local cache first (if not forcing refresh)
    if (!opts.forceRefresh) {
      const cached = await getLocalMetadata(game);
      if (cached) {
        return cached;
      }
    }
    
    // Query external source
    let metadata: GameMetadata | null = null;
    
    switch (opts.preferredSource) {
      case 'screenscraper':
        metadata = await lookupScreenscraper(game, opts);
        break;
      default:
        console.warn('Unknown metadata source');
    }
    
    // Cache the result
    if (metadata) {
      await saveLocalMetadata(game, metadata);
    }
    
    return metadata;
  } catch (error) {
    console.error('Failed to lookup metadata:', error);
    return null;
  }
}

/**
 * Lookup metadata from Screenscraper.fr
 * Requires user credentials (optional for limited lookups)
 */
async function lookupScreenscraper(
  game: Game,
  options: MetadataLookupOptions
): Promise<GameMetadata | null> {
  // Screenscraper requires authentication for most requests
  // For now, we'll use a basic search approach
  // In production, you'd want to store and use actual credentials
  
  const searchUrl = new URL(`${SCREENSCRAPER_BASE_URL}/api2/${SCREENSCRAPER_API_VERSION}/gameInfo.php`);
  searchUrl.searchParams.set('game_name', game.title);
  searchUrl.searchParams.set('system', 'snes');
  searchUrl.searchParams.set('region', 'usa');
  
  // Note: In a real implementation, you'd add:
  // searchUrl.searchParams.set('user', userId);
  // searchUrl.searchParams.set('pass', passwordHash);
  
  try {
    const response = await fetch(searchUrl.toString());
    
    if (!response.ok) {
      console.warn('Screenscraper lookup failed:', response.status);
      return null;
    }
    
    const data = await response.json();
    
    if (data.response && data.response.game) {
      const gameData = data.response.game;
      
      return {
        title: gameData.game_name || game.title,
        alternateTitles: gameData.alternate_names,
        description: gameData.description,
        releaseDate: gameData.release_date,
        developer: gameData.developer,
        publisher: gameData.publisher,
        genre: gameData.genre,
        players: gameData.players ? parseInt(gameData.players) : undefined,
        rating: gameData.rating ? parseFloat(gameData.rating) : undefined,
        coverUrl: options.includeCovers ? getScreenscraperMediaUrl(gameData, 'boxart') : undefined,
        thumbnailUrl: options.includeCovers ? getScreenscraperMediaUrl(gameData, 'thumbnails') : undefined,
        region: gameData.region,
      };
    }
    
    return null;
  } catch (error) {
    console.error('Screenscraper API error:', error);
    return null;
  }
}

/**
 * Get media URL from Screenscraper response
 */
function getScreenscraperMediaUrl(
  gameData: any,
  type: 'boxart' | 'thumbnails' | 'screenshots' | 'videos'
): string | undefined {
  const media = gameData.media;
  if (!media || !media[type]) {
    return undefined;
  }
  
  const mediaUrl = media[type];
  if (Array.isArray(mediaUrl)) {
    return mediaUrl[0]?.url;
  }
  
  return mediaUrl?.url;
}

/**
 * Get metadata from local cache
 */
async function getLocalMetadata(game: Game): Promise<GameMetadata | null> {
  // This would use Tauri's file system API in production
  // For now, we'll use localStorage as a fallback
  try {
    const cached = localStorage.getItem(`metadata_${game.id}`);
    if (cached) {
      return JSON.parse(cached);
    }
  } catch (error) {
    console.warn('Failed to read local metadata cache:', error);
  }
  return null;
}

/**
 * Save metadata to local cache
 */
async function saveLocalMetadata(game: Game, metadata: GameMetadata): Promise<void> {
  try {
    localStorage.setItem(`metadata_${game.id}`, JSON.stringify(metadata));
  } catch (error) {
    console.warn('Failed to save local metadata cache:', error);
  }
}

/**
 * Batch lookup metadata for multiple games
 */
export async function batchLookupMetadata(
  games: Game[],
  options: Partial<MetadataLookupOptions> = {},
  onProgress?: (completed: number, total: number) => void
): Promise<Map<string, GameMetadata | null>> {
  const results = new Map<string, GameMetadata | null>();
  const opts = { ...DEFAULT_OPTIONS, ...options };
  
  // Process in batches to avoid overwhelming the API
  const BATCH_SIZE = 5;
  
  for (let i = 0; i < games.length; i += BATCH_SIZE) {
    const batch = games.slice(i, i + BATCH_SIZE);
    
    const batchPromises = batch.map(async (game) => {
      const metadata = await lookupMetadata(game, opts);
      return { gameId: game.id, metadata };
    });
    
    const batchResults = await Promise.all(batchPromises);
    
    batchResults.forEach(({ gameId, metadata }) => {
      results.set(gameId, metadata);
    });
    
    if (onProgress) {
      onProgress(Math.min(i + BATCH_SIZE, games.length), games.length);
    }
    
    // Rate limiting - wait between batches
    if (i + BATCH_SIZE < games.length) {
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
  }
  
  return results;
}

/**
 * Clear metadata cache
 */
export async function clearMetadataCache(): Promise<void> {
  try {
    // Clear all metadata from localStorage
    const keysToRemove: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key?.startsWith('metadata_')) {
        keysToRemove.push(key);
      }
    }
    keysToRemove.forEach(key => localStorage.removeItem(key));
  } catch (error) {
    console.error('Failed to clear metadata cache:', error);
  }
}
