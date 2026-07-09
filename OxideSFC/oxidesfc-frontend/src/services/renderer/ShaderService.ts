/**
 * Shader Service for OxideSFC Frontend
 * 
 * Manages shader compilation, loading, and uniform management
 * for various rendering effects.
 */

import {
  PASSTHROUGH_VERT,
  PASSTHROUGH_FRAG,
  CRT_FRAG,
  XBRZ_FRAG,
  HQ2X_FRAG,
  SCALE2X_FRAG,
  POSTPROCESS_FRAG
} from './shaders';

export enum ShaderType {
  PASSTHROUGH = 'passthrough',
  CRT = 'crt',
  XBRZ = 'xbrz',
  HQ2X = 'hq2x',
  SCALE2X = 'scale2x',
  POSTPROCESS = 'postprocess',
}

interface ShaderProgram {
  program: WebGLProgram;
  uniforms: Record<string, WebGLUniformLocation | null>;
}

export class ShaderService {
  private gl: WebGLRenderingContext | WebGL2RenderingContext | null = null;
  private programs: Map<ShaderType, ShaderProgram> = new Map();
  private currentProgram: ShaderType = ShaderType.PASSTHROUGH;

  /**
   * Initialize shader service with WebGL context
   */
  async initialize(gl: WebGLRenderingContext | WebGL2RenderingContext): Promise<void> {
    this.gl = gl;
    await this.compileShaders();
  }

  /**
   * Compile all shader programs
   */
  private async compileShaders(): Promise<void> {
    if (!this.gl) return;

    // Compile passthrough shader
    await this.createProgram(
      ShaderType.PASSTHROUGH,
      PASSTHROUGH_VERT,
      PASSTHROUGH_FRAG
    );

    // Compile CRT shader
    await this.createProgram(
      ShaderType.CRT,
      PASSTHROUGH_VERT,
      CRT_FRAG
    );

    // Compile xBRZ shader
    await this.createProgram(
      ShaderType.XBRZ,
      PASSTHROUGH_VERT,
      XBRZ_FRAG
    );

    // Compile HQ2x shader
    await this.createProgram(
      ShaderType.HQ2X,
      PASSTHROUGH_VERT,
      HQ2X_FRAG
    );

    // Compile Scale2x shader
    await this.createProgram(
      ShaderType.SCALE2X,
      PASSTHROUGH_VERT,
      SCALE2X_FRAG
    );

    // Compile post-process shader
    await this.createProgram(
      ShaderType.POSTPROCESS,
      PASSTHROUGH_VERT,
      POSTPROCESS_FRAG
    );
  }

  /**
   * Create and compile a shader program
   */
  private async createProgram(
    type: ShaderType, 
    vertexSource: string, 
    fragmentSource: string
  ): Promise<boolean> {
    if (!this.gl) return false;

    // Compile vertex shader
    const vertexShader = this.compileShader(vertexSource, this.gl.VERTEX_SHADER);
    if (!vertexShader) {
      console.error(`Failed to compile vertex shader for ${type}`);
      return false;
    }

    // Compile fragment shader
    const fragmentShader = this.compileShader(fragmentSource, this.gl.FRAGMENT_SHADER);
    if (!fragmentShader) {
      console.error(`Failed to compile fragment shader for ${type}`);
      this.gl.deleteShader(vertexShader);
      return false;
    }

    // Create program
    const program = this.gl.createProgram();
    if (!program) {
      console.error(`Failed to create program for ${type}`);
      this.gl.deleteShader(vertexShader);
      this.gl.deleteShader(fragmentShader);
      return false;
    }

    // Attach shaders and link
    this.gl.attachShader(program, vertexShader);
    this.gl.attachShader(program, fragmentShader);
    this.gl.linkProgram(program);

    // Check for errors
    if (!this.gl.getProgramParameter(program, this.gl.LINK_STATUS)) {
      const error = this.gl.getProgramInfoLog(program);
      console.error(`Program link error for ${type}: ${error}`);
      this.gl.deleteProgram(program);
      this.gl.deleteShader(vertexShader);
      this.gl.deleteShader(fragmentShader);
      return false;
    }

    // Clean up shaders (they're now part of the program)
    this.gl.deleteShader(vertexShader);
    this.gl.deleteShader(fragmentShader);

    // Get uniform locations
    const uniforms = this.getUniformLocations(program);

    // Store program
    this.programs.set(type, { program, uniforms });
    console.log(`Shader compiled: ${type}`);
    
    return true;
  }

  /**
   * Compile a single shader
   */
  private compileShader(source: string, type: number): WebGLShader | null {
    if (!this.gl) return null;

    const shader = this.gl.createShader(type);
    if (!shader) return null;

    this.gl.shaderSource(shader, source);
    this.gl.compileShader(shader);

    if (!this.gl.getShaderParameter(shader, this.gl.COMPILE_STATUS)) {
      const error = this.gl.getShaderInfoLog(shader);
      console.error(`Shader compile error: ${error}`);
      this.gl.deleteShader(shader);
      return null;
    }

    return shader;
  }

  /**
   * Get all uniform locations for a program
   */
  private getUniformLocations(program: WebGLProgram): Record<string, WebGLUniformLocation | null> {
    if (!this.gl) return {};

    const uniforms: Record<string, WebGLUniformLocation | null> = {};
    const numUniforms = this.gl.getProgramParameter(program, this.gl.ACTIVE_UNIFORMS);

    for (let i = 0; i < numUniforms; i++) {
      const info = this.gl.getActiveUniform(program, i);
      if (info) {
        uniforms[info.name] = this.gl.getUniformLocation(program, info.name);
      }
    }

    return uniforms;
  }

  /**
   * Get a shader program by type
   */
  getProgram(type: ShaderType): WebGLProgram | null {
    const shaderProgram = this.programs.get(type);
    return shaderProgram ? shaderProgram.program : null;
  }

  /**
   * Get uniform location from current program
   */
  getUniform(name: string): WebGLUniformLocation | null {
    const shaderProgram = this.programs.get(this.currentProgram);
    return shaderProgram ? shaderProgram.uniforms[name] : null;
  }

  /**
   * Set the current shader program
   */
  useProgram(type: ShaderType): void {
    this.currentProgram = type;
    const shaderProgram = this.programs.get(type);
    if (this.gl && shaderProgram) {
      this.gl.useProgram(shaderProgram.program);
    }
  }

  /**
   * Set uniform value (float)
   */
  setUniformFloat(name: string, value: number): void {
    const location = this.getUniform(name);
    if (this.gl && location) {
      this.gl.uniform1f(location, value);
    }
  }

  /**
   * Set uniform value (vec2)
   */
  setUniformVec2(name: string, x: number, y: number): void {
    const location = this.getUniform(name);
    if (this.gl && location) {
      this.gl.uniform2f(location, x, y);
    }
  }

  /**
   * Set uniform value (vec3)
   */
  setUniformVec3(name: string, x: number, y: number, z: number): void {
    const location = this.getUniform(name);
    if (this.gl && location) {
      this.gl.uniform3f(location, x, y, z);
    }
  }

  /**
   * Set uniform value (vec4)
   */
  setUniformVec4(name: string, x: number, y: number, z: number, w: number): void {
    const location = this.getUniform(name);
    if (this.gl && location) {
      this.gl.uniform4f(location, x, y, z, w);
    }
  }

  /**
   * Set uniform value (int)
   */
  setUniformInt(name: string, value: number): void {
    const location = this.getUniform(name);
    if (this.gl && location) {
      this.gl.uniform1i(location, value);
    }
  }

  /**
   * Set uniform value (boolean)
   */
  setUniformBool(name: string, value: boolean): void {
    const location = this.getUniform(name);
    if (this.gl && location) {
      this.gl.uniform1i(location, value ? 1 : 0);
    }
  }

  /**
   * Get all available shader types
   */
  getAvailableShaders(): ShaderType[] {
    return Array.from(this.programs.keys());
  }

  /**
   * Check if a shader type is available
   */
  hasShader(type: ShaderType): boolean {
    return this.programs.has(type);
  }

  /**
   * Dispose all shader resources
   */
  dispose(): void {
    if (!this.gl) return;

    for (const [type, shaderProgram] of this.programs) {
      this.gl.deleteProgram(shaderProgram.program);
      this.programs.delete(type);
    }

    this.gl = null;
  }
}