/**
 * IGDB Client
 * 
 * Client for the IGDB (Internet Games Database) API to fetch game metadata.
 * 
 * API Documentation: https://api-docs.igdb.com/
 */

import { APIClient, APIError, APIClientConfig, DEFAULT_API_CONFIG } from './APIClient';
import type { GameMetadata } from '../../domain/types';

// ============================================================================
// IGDB Types
// ============================================================================

/**
 * IGDB game response
 */
interface IGDBGame {
  id: number;
  name: string;
  summary?: string;
  storyline?: string;
  first_release_date?: number;
  rating?: number;
  aggregated_rating?: number;
  cover?: {
    image_id: string;
  };
  genres?: { name: string }[];
  companies?: {
    company: { name: string };
    developer: boolean;
    publisher: boolean;
  }[];
  multiplayer_modes?: {
    onlinecoop: boolean;
    splitscreen: boolean;
  }[];
  websites?: {
    url: string;
    category: number;
  }[];
}

/**
 * IGDB search response
 */
type IGDBResponse = IGDBGame[];

/**
 * IGDB client configuration
 */
export interface IGDBConfig extends Partial<APIClientConfig> {
  clientId: string;
  clientSecret: string;
}

// ============================================================================
// Default Configuration
// ============================================================================

const IGDB_CONFIG: Partial<APIClientConfig> = {
  ...DEFAULT_API_CONFIG,
  baseUrl: 'https://api.igdb.com/v4',
  timeout: 15000,
  enableCache: true,
  cacheDuration: 24 * 60 * 60 * 1000, // 24 hours
  enableRateLimit: true,
  maxRequests: 4,
  rateLimitWindow: 1000, // 1 second for IGDB
  enableRetry: true,
  maxRetries: 3,
  retryDelay: 2000,
};

// ============================================================================
// IGDB Client Implementation
// ============================================================================

/**
 * IGDB API Client
 * 
 * Provides methods for searching games and fetching metadata
 * from the IGDB database.
 */
export class IGDBClient extends APIClient<IGDBResponse> {
  private clientId: string;
  private clientSecret: string;
  private accessToken: string | null = null;
  private tokenExpiry: number = 0;

  constructor(config: IGDBConfig) {
    super(IGDB_CONFIG);
    
    this.clientId = config.clientId;
    this.clientSecret = config.clientSecret;
  }

  /**
   * Execute the actual HTTP request
   */
  protected async executeRequest(
    method: string,
    url: string,
    body?: string
  ): Promise<IGDBResponse> {
    // Ensure we have a valid access token
    await this.ensureToken();

    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.config.timeout);

    try {
      const response = await fetch(url, {
        method,
        headers: {
          'Client-ID': this.clientId,
          'Authorization': `Bearer ${this.accessToken}`,
          'Content-Type': 'text/plain',
        },
        // body is already a raw Apicalypse query string (see searchByName/
        // getGameById below) — it must be sent as-is, not JSON-encoded,
        // to match the 'text/plain' content type IGDB expects.
        body,
        signal: controller.signal,
      });

      clearTimeout(timeoutId);

      // Handle rate limiting
      if (response.status === 429) {
        throw new APIError('Rate limited by IGDB', 429, { isRateLimited: true });
      }

      return this.parseResponse<IGDBResponse>(response);
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
   * Ensure we have a valid access token
   */
  private async ensureToken(): Promise<void> {
    if (this.accessToken && Date.now() < this.tokenExpiry) {
      return;
    }

    // Get new token
    const tokenUrl = 'https://id.twitch.tv/oauth2/token';
    const params = new URLSearchParams({
      client_id: this.clientId,
      client_secret: this.clientSecret,
      grant_type: 'client_credentials',
    });

    const response = await fetch(tokenUrl, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/x-www-form-urlencoded',
      },
      body: params,
    });

    if (!response.ok) {
      throw new APIError('Failed to obtain IGDB access token', response.status);
    }

    const tokenData = await response.json() as {
      access_token: string;
      expires_in: number;
    };

    this.accessToken = tokenData.access_token;
    this.tokenExpiry = Date.now() + (tokenData.expires_in * 1000) - 60000; // Subtract 1 minute buffer
  }

  /**
   * Search for games by name
   * @param name - Game name to search for
   * @param limit - Maximum number of results
   * @returns Array of game metadata
   */
  async searchByName(name: string, limit: number = 10): Promise<GameMetadata[]> {
    const query = `
      search "${name}";
      fields id,name,summary,storyline,first_release_date,rating,aggregated_rating,
             cover.image_id,genres.name,companies.company.name,companies.developer,
             companies.publisher,multiplayer_modes.onlinecoop,multiplayer_modes.splitscreen;
      limit ${limit};
    `;

    const response = await this.post('/games', query);
    return this.parseSearchResponse(response);
  }

  /**
   * Get game by IGDB ID
   * @param id - IGDB game ID
   * @returns Game metadata
   */
  async getGameById(id: number): Promise<GameMetadata | null> {
    const query = `
      fields id,name,summary,storyline,first_release_date,rating,aggregated_rating,
             cover.image_id,genres.name,companies.company.name,companies.developer,
             companies.publisher,multiplayer_modes.onlinecoop,multiplayer_modes.splitscreen;
      where id = ${id};
    `;

    const response = await this.post('/games', query);
    
    if (response.length === 0) {
      return null;
    }
    
    return this.parseSearchResponse(response)[0];
  }

  /**
   * Get game cover URL
   * @param imageId - IGDB image ID
   * @param size - Image size (cover_small, cover_big, cover_large)
   * @returns URL to the cover image
   */
  getCoverUrl(imageId: string, size: 'cover_small' | 'cover_big' | 'cover_large' = 'cover_big'): string {
    return `https://images.igdb.com/igdb/image/upload/t_${size}/${imageId}.jpg`;
  }

  /**
   * Parse IGDB response to GameMetadata
   */
  private parseSearchResponse(response: IGDBResponse): GameMetadata[] {
    return response.map(game => {
      // Find developer and publisher
      const developer = game.companies?.find(c => c.developer)?.company.name;
      const publisher = game.companies?.find(c => c.publisher)?.company.name;
      
      // Convert release date
      let releaseDate: string | null = null;
      if (game.first_release_date) {
        const date = new Date(game.first_release_date * 1000);
        releaseDate = date.toISOString().split('T')[0];
      }

      return {
        game_id: game.id.toString(),
        title: game.name,
        alternate_titles: [],
        description: game.summary ?? game.storyline ?? '',
        release_date: releaseDate,
        developer: developer ?? null,
        publisher: publisher ?? null,
        genre: game.genres?.[0]?.name ?? null,
        players: this.parsePlayers(game),
        rating: game.rating ?? game.aggregated_rating ?? null,
        cover_url: game.cover ? this.getCoverUrl(game.cover.image_id) : null,
        source: 'igdb' as const,
      };
    });
  }

  /**
   * Parse player count from multiplayer modes
   */
  private parsePlayers(game: IGDBGame): number {
    const modes = game.multiplayer_modes?.[0];
    
    if (modes?.onlinecoop || modes?.splitscreen) {
      return 2; // Assume at least 2 for multiplayer modes
    }
    
    return 1;
  }

  /**
   * Check if client credentials are configured
   */
  hasCredentials(): boolean {
    return !!(this.clientId && this.clientSecret);
  }
}

// ============================================================================
// Factory Function
// ============================================================================

/**
 * Create a new IGDB client
 */
export function createIGDBClient(config: IGDBConfig): IGDBClient {
  return new IGDBClient(config);
}
