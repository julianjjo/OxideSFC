/**
 * WebGL Renderer Module for OxideSFC Frontend
 * 
 * Exports all renderer components and shader services.
 */

export { WebGLRenderer } from './WebGLRenderer';
export type { RenderOptions, RendererStats } from './WebGLRenderer';

export { ShaderService } from './ShaderService';
export { ShaderType } from './ShaderService';

// Re-export shaders for convenience
export * from './shaders';