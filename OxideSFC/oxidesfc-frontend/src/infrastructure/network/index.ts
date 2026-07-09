/**
 * Network Infrastructure Module
 * 
 * Provides API clients for external services:
 * - Base APIClient with caching and rate limiting
 * - ScreenscraperClient for game metadata
 * - IGDBClient for game database lookups
 */

export { APIClient, APIError } from './APIClient';
export type { APIClientConfig, APIError as APIClientError } from './APIClient';
export { DEFAULT_API_CONFIG } from './APIClient';

export { ScreenscraperClient, createScreenscraperClient } from './ScreenscraperClient';
export type { ScreenscraperConfig } from './ScreenscraperClient';

export { IGDBClient, createIGDBClient } from './IGDBClient';
export type { IGDBConfig } from './IGDBClient';
