/**
 * Screenscraper Client
 * 
 * Client for the ScreenScraper API to fetch game metadata,
 * box art, and screenshots.
 * 
 * API Documentation: https://www.screenscraper.fr/
 */

import { APIClient, APIError, APIClientConfig, DEFAULT_API_CONFIG } from './APIClient';
import type { GameMetadata } from '../../domain/types';

// ============================================================================
// Screenscraper Types
// ============================================================================

/**
 * Screenscraper game response
 */
interface ScreenscraperGame {
  id: string;
  nom?: string;
  nom2?: string;
  synopsis?: string;
  dates?: { date?: string }[];
  developers?: { nom?: string }[];
  publishers?: { nom?: string }[];
  genres?: { nom?: string }[];
  players?: string;
  note?: string;
  medias?: {
    box2d?: { url: string }[];
    box3d?: { url: string }[];
    screenshot?: { url: string }[];
    video?: { url: string }[];
    titlescreen?: { url: string }[];
  };
}

/**
 * Screenscraper search response
 */
interface ScreenscraperSearchResponse {
  response: {
    jeu?: ScreenscraperGame | ScreenscraperGame[];
    erreur?: string;
  };
}

/**
 * Screenscraper ROM info request
 */
interface ScreenscraperRomInfo {
  md5?: string;
  sha1?: string;
  crc?: string;
  nom: string;
  taille: number;
  type?: string;
}

/**
 * Screenscraper user credentials
 */
interface ScreenscraperCredentials {
  dev_id: string;
  soft_id: string;
  user_id?: string;
  user_password?: string;
}

/**
 * Screenscraper client configuration
 */
export interface ScreenscraperConfig extends Partial<APIClientConfig> {
  devId: string;
  softId: string;
  userId?: string;
  userPassword?: string;
}

// ============================================================================
// Default Configuration
// ============================================================================

const SCREENSCRAPER_CONFIG: Partial<APIClientConfig> = {
  ...DEFAULT_API_CONFIG,
  baseUrl: 'https://www.screenscraper.fr/api2',
  timeout: 30000,
  enableCache: true,
  cacheDuration: 24 * 60 * 60 * 1000, // 24 hours
  enableRateLimit: true,
  maxRequests: 10,
  rateLimitWindow: 60 * 1000,
  enableRetry: true,
  maxRetries: 3,
  retryDelay: 2000,
};

// ============================================================================
// Screenscraper Client Implementation
// ============================================================================

/**
 * Screenscraper API Client
 * 
 * Provides methods for searching games and fetching metadata
 * from the ScreenScraper database.
 */
export class ScreenscraperClient extends APIClient<ScreenscraperSearchResponse> {
  private credentials: ScreenscraperCredentials;

  constructor(config: ScreenscraperConfig) {
    super(SCREENSCRAPER_CONFIG);
    
    this.credentials = {
      dev_id: config.devId,
      soft_id: config.softId,
      user_id: config.userId,
      user_password: config.userPassword,
    };
  }

  /**
   * Execute the actual HTTP request
   */
  protected async executeRequest(
    method: string,
    url: string,
    body?: unknown
  ): Promise<ScreenscraperSearchResponse> {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.config.timeout);

    try {
      const response = await fetch(url, {
        method,
        headers: this.createHeaders(),
        body: body ? JSON.stringify(body) : undefined,
        signal: controller.signal,
      });

      clearTimeout(timeoutId);

      // Handle rate limiting
      if (response.status === 429) {
        throw new APIError('Rate limited by ScreenScraper', 429, { isRateLimited: true });
      }

      return this.parseResponse<ScreenscraperSearchResponse>(response);
    } catch (error) {
      clearTimeout(timeoutId);
      
      if (error instanceof APIError) {
        throw error;
      }
      
      if (error instanceof Error) {
        if (error.name === 'AbortError') {
          throw new APIError('Request timeout', null, { isTimeout: true });
        }
        throw new APIError(error.message, null, { isNetworkError: true });
      }
      
      throw new APIError('Unknown error', null, { isNetworkError: true });
    }
  }

  /**
   * Search for a game by ROM info
   * @param romInfo - ROM file information (md5, sha1, crc, name, size)
   * @returns Game metadata or null if not found
   */
  async searchByRom(romInfo: ScreenscraperRomInfo): Promise<GameMetadata | null> {
    const params = {
      ...this.credentials,
      JeuxNomSearch: romInfo.nom,
      rom: JSON.stringify(romInfo),
    };

    const response = await this.get('/jeuRecherche.php', params);
    return this.parseSearchResponse(response);
  }

  /**
   * Search for a game by name
   * @param name - Game name to search for
   * @param systemId - Optional system ID to filter by
   * @returns Game metadata or null if not found
   */
  async searchByName(name: string, systemId?: number): Promise<GameMetadata | null> {
    const params: Record<string, string> = {
      ...this.credentials,
      JeuxNomSearch: name,
    };

    if (systemId) {
      params.systemeid = systemId.toString();
    }

    const response = await this.get('/jeuRecherche.php', params);
    return this.parseSearchResponse(response);
  }

  /**
   * Get game details by ID
   * @param gameId - ScreenScraper game ID
   * @returns Game metadata
   */
  async getGameById(gameId: string): Promise<GameMetadata | null> {
    const params = {
      ...this.credentials,
      jeuid: gameId,
    };

    const response = await this.get('/jeuInfos.php', params);
    return this.parseSearchResponse(response);
  }

  /**
   * Get box art URL for a game
   * @param gameId - ScreenScraper game ID
   * @param type - Box art type (box2d, box3d)
   * @returns URL or null if not available
   */
  async getBoxArt(gameId: string, type: 'box2d' | 'box3d' = 'box2d'): Promise<string | null> {
    const game = await this.getGameById(gameId);

    if (!game) return null;

    return type === 'box3d' ? (game.cover_url_3d ?? null) : game.cover_url;
  }

  /**
   * Parse ScreenScraper search response
   */
  private parseSearchResponse(response: ScreenscraperSearchResponse): GameMetadata | null {
    const data = response.response;
    
    if (data.erreur) {
      throw new APIError(data.erreur);
    }

    // Handle array of games
    let game: ScreenscraperGame | undefined;
    if (Array.isArray(data.jeu)) {
      game = data.jeu[0];
    } else if (data.jeu) {
      game = data.jeu;
    }

    if (!game) {
      return null;
    }

    // Parse game data
    const metadata: GameMetadata = {
      game_id: game.id,
      title: game.nom ?? 'Unknown',
      alternate_titles: game.nom2 ? [game.nom2] : [],
      description: game.synopsis ?? '',
      release_date: game.dates?.[0]?.date ?? null,
      developer: game.developers?.[0]?.nom ?? null,
      publisher: game.publishers?.[0]?.nom ?? null,
      genre: game.genres?.[0]?.nom ?? null,
      players: parseInt(game.players ?? '1', 10) || 1,
      rating: game.note ? parseFloat(game.note) / 20 : null,
      cover_url: game.medias?.box2d?.[0]?.url ?? null,
      cover_url_3d: game.medias?.box3d?.[0]?.url ?? null,
      source: 'screenscraper',
    };

    return metadata;
  }

  /**
   * Check if user credentials are configured
   */
  hasCredentials(): boolean {
    return !!(this.credentials.user_id && this.credentials.user_password);
  }
}

// ============================================================================
// Factory Function
// ============================================================================

/**
 * Create a new Screenscraper client
 */
export function createScreenscraperClient(config: ScreenscraperConfig): ScreenscraperClient {
  return new ScreenscraperClient(config);
}
