/**
 * API Client Base Class
 * 
 * Base class for network clients with:
 * - Error handling
 * - Request caching
 * - Rate limiting
 * - Retry logic
 */

// ============================================================================
// API Client Types
// ============================================================================

/**
 * API client configuration
 */
export interface APIClientConfig {
  /**
   * Base URL for API requests
   */
  baseUrl: string;
  
  /**
   * Request timeout in ms
   */
  timeout: number;
  
  /**
   * Enable caching
   */
  enableCache: boolean;
  
  /**
   * Cache duration in ms
   */
  cacheDuration: number;
  
  /**
   * Enable rate limiting
   */
  enableRateLimit: boolean;
  
  /**
   * Maximum requests per time window
   */
  maxRequests: number;
  
  /**
   * Rate limit window in ms
   */
  rateLimitWindow: number;
  
  /**
   * Enable retry logic
   */
  enableRetry: boolean;
  
  /**
   * Maximum retry attempts
   */
  maxRetries: number;
  
  /**
   * Retry delay in ms
   */
  retryDelay: number;
}

/**
 * Default API client configuration
 */
export const DEFAULT_API_CONFIG: APIClientConfig = {
  baseUrl: '',
  timeout: 10000,
  enableCache: true,
  cacheDuration: 5 * 60 * 1000, // 5 minutes
  enableRateLimit: true,
  maxRequests: 10,
  rateLimitWindow: 60 * 1000, // 1 minute
  enableRetry: true,
  maxRetries: 3,
  retryDelay: 1000,
};

/**
 * API error
 */
export class APIError extends Error {
  public statusCode: number | null;
  public isNetworkError: boolean;
  public isTimeout: boolean;
  public isRateLimited: boolean;

  constructor(
    message: string,
    statusCode: number | null = null,
    options?: {
      isNetworkError?: boolean;
      isTimeout?: boolean;
      isRateLimited?: boolean;
    }
  ) {
    super(message);
    this.name = 'APIError';
    this.statusCode = statusCode;
    this.isNetworkError = options?.isNetworkError ?? false;
    this.isTimeout = options?.isTimeout ?? false;
    this.isRateLimited = options?.isRateLimited ?? false;
  }
}

// ============================================================================
// Cache Implementation
// ============================================================================

interface CacheEntry<T> {
  data: T;
  timestamp: number;
  expiresAt: number;
}

// ============================================================================
// Rate Limiter Implementation
// ============================================================================

class RateLimiter {
  private requestTimestamps: number[] = [];
  private config: APIClientConfig;

  constructor(config: APIClientConfig) {
    this.config = config;
  }

  /**
   * Wait for rate limit
   */
  async wait(): Promise<void> {
    if (!this.config.enableRateLimit) return;

    const now = Date.now();
    const windowStart = now - this.config.rateLimitWindow;

    // Remove old timestamps
    this.requestTimestamps = this.requestTimestamps.filter(ts => ts > windowStart);

    // Check if we've hit the limit
    if (this.requestTimestamps.length >= this.config.maxRequests) {
      const oldestTimestamp = this.requestTimestamps[0];
      const waitTime = oldestTimestamp + this.config.rateLimitWindow - now;
      
      if (waitTime > 0) {
        await new Promise(resolve => setTimeout(resolve, waitTime));
      }
    }

    // Add current timestamp
    this.requestTimestamps.push(Date.now());
  }

  /**
   * Get current usage
   */
  getUsage(): { current: number; limit: number } {
    const now = Date.now();
    const windowStart = now - this.config.rateLimitWindow;
    const currentCount = this.requestTimestamps.filter(ts => ts > windowStart).length;
    
    return {
      current: currentCount,
      limit: this.config.maxRequests,
    };
  }
}

// ============================================================================
// API Client Base Class
// ============================================================================

/**
 * Base API Client
 * 
 * Provides common functionality for making HTTP requests with
 * caching, rate limiting, and retry logic.
 */
export abstract class APIClient<TResponse = unknown> {
  protected config: APIClientConfig;
  private cache: Map<string, CacheEntry<unknown>> = new Map();
  private rateLimiter: RateLimiter;

  constructor(config: Partial<APIClientConfig> = {}) {
    this.config = { ...DEFAULT_API_CONFIG, ...config };
    this.rateLimiter = new RateLimiter(this.config);
  }

  // ==========================================================================
  // HTTP Methods
  // ==========================================================================

  /**
   * Make a GET request
   */
  async get(endpoint: string, params?: Record<string, string>): Promise<TResponse> {
    const url = this.buildUrl(endpoint, params);
    return this.request('GET', url);
  }

  /**
   * Make a POST request
   */
  async post(endpoint: string, data?: unknown): Promise<TResponse> {
    const url = this.buildUrl(endpoint);
    return this.request('POST', url, data);
  }

  /**
   * Make a PUT request
   */
  async put(endpoint: string, data?: unknown): Promise<TResponse> {
    const url = this.buildUrl(endpoint);
    return this.request('PUT', url, data);
  }

  /**
   * Make a DELETE request
   */
  async delete(endpoint: string): Promise<TResponse> {
    const url = this.buildUrl(endpoint);
    return this.request('DELETE', url);
  }

  // ==========================================================================
  // Core Request Methods
  // ==========================================================================

  /**
   * Make an HTTP request
   */
  protected async request(
    method: string,
    url: string,
    body?: unknown,
    retryCount: number = 0
  ): Promise<TResponse> {
    // Check cache for GET requests
    if (method === 'GET' && this.config.enableCache) {
      const cached = this.getCached<TResponse>(url);
      if (cached) {
        return cached;
      }
    }

    // Wait for rate limit
    await this.rateLimiter.wait();

    try {
      const response = await this.executeRequest(method, url, body);
      
      // Cache successful GET responses
      if (method === 'GET' && this.config.enableCache && response) {
        this.setCache(url, response);
      }
      
      return response;
    } catch (error) {
      // Handle rate limiting
      if (error instanceof APIError && error.isRateLimited) {
        if (this.config.enableRetry && retryCount < this.config.maxRetries) {
          await this.delay(this.config.retryDelay * (retryCount + 1));
          return this.request(method, url, body, retryCount + 1);
        }
      }

      // Handle network errors and timeouts
      if (error instanceof APIError && (error.isNetworkError || error.isTimeout)) {
        if (this.config.enableRetry && retryCount < this.config.maxRetries) {
          await this.delay(this.config.retryDelay * (retryCount + 1));
          return this.request(method, url, body, retryCount + 1);
        }
      }

      throw error;
    }
  }

  /**
   * Execute the actual HTTP request
   */
  protected abstract executeRequest(
    method: string,
    url: string,
    body?: unknown
  ): Promise<TResponse>;

  /**
   * Build URL with query parameters
   */
  protected buildUrl(endpoint: string, params?: Record<string, string>): string {
    let url = `${this.config.baseUrl}${endpoint}`;
    
    if (params && Object.keys(params).length > 0) {
      const searchParams = new URLSearchParams(params);
      url += `?${searchParams.toString()}`;
    }
    
    return url;
  }

  // ==========================================================================
  // Cache Methods
  // ==========================================================================

  /**
   * Get cached data
   */
  protected getCached<T>(url: string): T | null {
    const entry = this.cache.get(url) as CacheEntry<T> | undefined;
    
    if (entry && entry.expiresAt > Date.now()) {
      return entry.data;
    }
    
    // Remove expired entry
    if (entry) {
      this.cache.delete(url);
    }
    
    return null;
  }

  /**
   * Set cache entry
   */
  protected setCache(url: string, data: unknown): void {
    const entry: CacheEntry<unknown> = {
      data,
      timestamp: Date.now(),
      expiresAt: Date.now() + this.config.cacheDuration,
    };
    
    this.cache.set(url, entry);
  }

  /**
   * Clear cache
   */
  clearCache(): void {
    this.cache.clear();
  }

  /**
   * Get cache usage
   */
  getCacheSize(): number {
    return this.cache.size;
  }

  // ==========================================================================
  // Rate Limiter Methods
  // ==========================================================================

  /**
   * Get rate limiter usage
   */
  getRateLimitUsage(): { current: number; limit: number } {
    return this.rateLimiter.getUsage();
  }

  // ==========================================================================
  // Utility Methods
  // ==========================================================================

  /**
   * Parse JSON response
   */
  protected parseResponse<T>(response: Response): Promise<T> {
    if (!response.ok) {
      throw new APIError(
        `HTTP ${response.status}: ${response.statusText}`,
        response.status
      );
    }
    
    return response.json() as Promise<T>;
  }

  /**
   * Create fetch headers
   */
  protected createHeaders(additionalHeaders?: Record<string, string>): Headers {
    const headers = new Headers({
      'Content-Type': 'application/json',
      ...additionalHeaders,
    });
    
    return headers;
  }

  /**
   * Delay helper
   */
  protected delay(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  /**
   * Update configuration
   */
  updateConfig(config: Partial<APIClientConfig>): void {
    this.config = { ...this.config, ...config };
  }
}
