/**
 * Infrastructure Layer
 *
 * Core infrastructure services for the OxideSFC frontend:
 * - Emulation: Tauri-based emulation core interface
 * - Filesystem: File dialogs and ROM/save management
 * - Network: API clients for metadata fetching
 */

// ============================================================================
// Emulation Infrastructure
// ============================================================================

export * from './emulation';

// ============================================================================
// Filesystem Infrastructure
// ============================================================================

export * from './filesystem';

// ============================================================================
// Network Infrastructure
// ============================================================================

export * from './network';
