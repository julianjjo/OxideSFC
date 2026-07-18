/**
 * WebGL Renderer for OxideSFC Frontend
 * 
 * Provides hardware-accelerated rendering with shader support for
 * CRT effects and various upscaling algorithms.
 */

import { ShaderService, ShaderType } from './ShaderService';

export interface RenderOptions {
  width: number;
  height: number;
  scaleMode: 'nearest' | 'bilinear' | 'xbrz' | 'hq2x';
  crtMode: boolean;
  shader?: string;
}

export interface RendererStats {
  fps: number;
  frameTime: number;
  drawCalls: number;
}

export class WebGLRenderer {
  private canvas: HTMLCanvasElement;
  private gl: WebGL2RenderingContext | WebGLRenderingContext | null = null;
  private shaderService: ShaderService;
  
  // WebGL resources
  private texture: WebGLTexture | null = null;
  private framebuffer: WebGLFramebuffer | null = null;
  private vertexBuffer: WebGLBuffer | null = null;
  private program: WebGLProgram | null = null;
  
  // State
  private isInitialized: boolean = false;
  private options: RenderOptions;

  // Bound event handlers (stored so dispose() can remove the exact same
  // function references that were registered)
  private handleContextLost = (e: Event): void => {
    e.preventDefault();
    this.contextLost = true;
    console.warn('WebGL context lost');
  };

  private handleContextRestored = (): void => {
    console.log('WebGL context restored');
    this.contextLost = false;
    this.reinitialize();
  };
  
  // Performance tracking
  private lastFrameTime: number = 0;
  private frameCount: number = 0;
  private fps: number = 0;
  private fpsUpdateTime: number = 0;
  
  // Context loss handling
  private contextLost: boolean = false;

  constructor(canvas: HTMLCanvasElement, options: Partial<RenderOptions> = {}) {
    this.canvas = canvas;
    this.options = {
      width: 512,
      height: 480,
      scaleMode: 'nearest',
      crtMode: false,
      ...options,
    };
    this.shaderService = new ShaderService();
  }

  /**
   * Initialize the WebGL context and resources
   */
  async initialize(): Promise<boolean> {
    // Try WebGL2 first, fall back to WebGL1
    const contextOptions: WebGLContextAttributes = {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      // Screenshots (QuickMenu's "Screenshot" action, see
      // src/components/emulator/QuickMenu.tsx) call canvas.toBlob()/
      // toDataURL() on this same canvas element from a user-triggered
      // click, at an arbitrary point in time relative to the render loop.
      // With preserveDrawingBuffer left false (the GPU-friendlier default),
      // the drawing buffer can be cleared immediately after the frame is
      // presented to the compositor, so a toBlob() call made even a few
      // milliseconds after the last draw can capture a blank/black canvas.
      // Setting this true keeps the buffer intact between frames so
      // screenshots are reliably non-blank; the perf cost is negligible at
      // this 512x480 SNES output resolution.
      preserveDrawingBuffer: true,
      powerPreference: 'high-performance',
    };

    // Try WebGL2
    this.gl = this.canvas.getContext('webgl2', contextOptions) as WebGL2RenderingContext;
    
    if (!this.gl) {
      // Fall back to WebGL1
      this.gl = this.canvas.getContext('webgl', contextOptions) as WebGLRenderingContext;
      
      if (!this.gl) {
        console.error('WebGL not supported');
        return false;
      }
      
      console.log('Using WebGL1 (fallback)');
    } else {
      console.log('Using WebGL2');
    }

    // Setup context loss handling
    this.setupContextLossHandling();

    // Initialize shader service
    await this.shaderService.initialize(this.gl);

    // Create WebGL resources
    if (!this.createResources()) {
      return false;
    }

    this.isInitialized = true;
    return true;
  }

  /**
   * Setup context loss/restore handlers
   */
  private setupContextLossHandling(): void {
    if (!this.gl) return;

    this.canvas.addEventListener('webglcontextlost', this.handleContextLost);
    this.canvas.addEventListener('webglcontextrestored', this.handleContextRestored);
  }

  /**
   * Reinitialize resources after context restore
   */
  private async reinitialize(): Promise<void> {
    if (!this.gl) return;

    await this.shaderService.initialize(this.gl);
    this.createResources();

    // createResources() already re-selects the shader that matches
    // this.options (via resolveShaderType), so the user's chosen CRT/xBRZ/
    // HQ2x filter survives a context loss instead of reverting to
    // passthrough. Canvas pixel dimensions are owned by the view (see
    // EmulatorView's ResizeObserver) and survive a context loss untouched.

    this.contextLost = false;
  }

  /**
   * Create WebGL resources (buffers, textures, etc.)
   */
  private createResources(): boolean {
    const gl = this.gl;
    if (!gl) return false;

    // Create vertex buffer for full-screen quad. Texcoord V is flipped
    // relative to screen position (bottom of screen <-> v=1, top <-> v=0):
    // `texImage2D` uploads our RGBA buffer's row 0 (the emulator's first
    // rendered scanline, i.e. the top of the game screen) to texture v=0,
    // so the quad must sample v=0 at the *top* of the screen to avoid
    // rendering the whole frame upside down.
    const vertices = new Float32Array([
      -1, -1, 0, 1,
       1, -1, 1, 1,
      -1,  1, 0, 0,
       1,  1, 1, 0,
    ]);

    this.vertexBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vertexBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);

    // Create texture for video frame
    this.texture = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    // Match the default scaleMode ('nearest') so the first frames aren't
    // bilinear-smoothed before setOptions() runs.
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);

    // Create framebuffer for multi-pass rendering
    this.framebuffer = gl.createFramebuffer();

    // Set up initial program based on the currently selected filter (falls
    // back to passthrough via resolveShaderType when no filter is active).
    this.program = this.shaderService.getProgram(this.resolveShaderType());
    if (!this.program) {
      console.error('Failed to load shader program');
      return false;
    }

    return true;
  }

  /**
   * Resolve which shader should be active based on the current options.
   * This is the single source of truth for "what filter did the user pick" -
   * used both when options change and when resources are (re)created after
   * a context restore, so a restore doesn't silently revert to passthrough.
   */
  private resolveShaderType(): ShaderType {
    if (this.options.crtMode) {
      return ShaderType.CRT;
    } else if (this.options.scaleMode === 'xbrz') {
      return ShaderType.XBRZ;
    } else if (this.options.scaleMode === 'hq2x') {
      return ShaderType.HQ2X;
    }
    return ShaderType.PASSTHROUGH;
  }

  /**
   * Update rendering options
   */
  setOptions(options: Partial<RenderOptions>): void {
    this.options = { ...this.options, ...options };

    // Update shader based on options
    this.program = this.shaderService.getProgram(this.resolveShaderType());

    // Update texture filtering
    this.updateTextureFiltering();
  }

  /**
   * Update texture filtering based on scale mode
   */
  private updateTextureFiltering(): void {
    const gl = this.gl;
    if (!gl || !this.texture) return;

    const filter = this.options.scaleMode === 'nearest' 
      ? gl.NEAREST 
      : gl.LINEAR;

    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
  }

  /**
   * Render a video frame to the canvas
   */
  render(frameData: Uint8Array | Uint8ClampedArray, width: number, height: number): void {
    if (!this.isInitialized || this.contextLost || !this.gl || !this.program) {
      return;
    }

    const gl = this.gl;
    const startTime = performance.now();

    // NOTE: the canvas's drawing-buffer size (canvas.width/height) is owned
    // by the view layer (EmulatorView sizes it from a ResizeObserver +
    // devicePixelRatio, letterboxed to the frame's aspect ratio). Driving it
    // from here based on parentElement measurements caused a layout feedback
    // loop: the buffer's intrinsic size participates in flex layout
    // (min-height:auto), which grew the container the next measurement read
    // from, pushing the UI bars off-screen until a window resize forced a
    // clean re-layout.

    // Upload frame data to texture
    this.uploadTexture(frameData, width, height);

    // Set viewport
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    // Use shader program
    gl.useProgram(this.program);

    // Set uniforms
    this.setUniforms(width, height);

    // Bind texture
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    
    // Set texture uniform
    const uTexture = gl.getUniformLocation(this.program, 'uTexture');
    if (uTexture) {
      gl.uniform1i(uTexture, 0);
    }

    // Bind vertex buffer
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vertexBuffer);
    
    // Set vertex attributes
    const aPosition = gl.getAttribLocation(this.program, 'aPosition');
    const aTexCoord = gl.getAttribLocation(this.program, 'aTexCoord');
    
    if (aPosition !== -1) {
      gl.enableVertexAttribArray(aPosition);
      gl.vertexAttribPointer(aPosition, 2, gl.FLOAT, false, 16, 0);
    }
    
    if (aTexCoord !== -1) {
      gl.enableVertexAttribArray(aTexCoord);
      gl.vertexAttribPointer(aTexCoord, 2, gl.FLOAT, false, 16, 8);
    }

    // Draw
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    // Update performance stats
    this.updateStats(startTime);
  }

  /**
   * Upload frame data to GPU texture
   */
  private uploadTexture(data: Uint8Array | Uint8ClampedArray, width: number, height: number): void {
    const gl = this.gl;
    if (!gl || !this.texture) return;

    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    
    // Check if data is already in the right format
    if (data instanceof Uint8ClampedArray) {
      // Convert to Uint8Array for texImage2D
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array(data.buffer));
    } else {
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, data);
    }
  }

  /**
   * Set shader uniforms
   */
  private setUniforms(width: number, height: number): void {
    const gl = this.gl;
    if (!gl || !this.program) return;

    // Resolution
    const uResolution = gl.getUniformLocation(this.program, 'uResolution');
    if (uResolution) {
      gl.uniform2f(uResolution, width, height);
    }

    // Time (for CRT effects)
    const uTime = gl.getUniformLocation(this.program, 'uTime');
    if (uTime) {
      gl.uniform1f(uTime, performance.now() / 1000);
    }

    // CRT-specific uniforms
    if (this.options.crtMode) {
      const uCurvature = gl.getUniformLocation(this.program, 'uCurvature');
      if (uCurvature) {
        gl.uniform1f(uCurvature, 3.0);
      }

      const uScanlineIntensity = gl.getUniformLocation(this.program, 'uScanlineIntensity');
      if (uScanlineIntensity) {
        gl.uniform1f(uScanlineIntensity, 0.3);
      }

      const uVignetteStrength = gl.getUniformLocation(this.program, 'uVignetteStrength');
      if (uVignetteStrength) {
        gl.uniform1f(uVignetteStrength, 0.5);
      }

      const uChromaticAberration = gl.getUniformLocation(this.program, 'uChromaticAberration');
      if (uChromaticAberration) {
        gl.uniform1f(uChromaticAberration, 0.003);
      }

      // The CRT fragment shader multiplies/offsets color by these before
      // applying scanlines/vignette (color *= uBrightness, etc.). WebGL
      // uniforms default to 0 when never set, which silently forced the
      // whole CRT-mode output to black. RenderOptions doesn't currently
      // expose brightness/contrast/saturation as user-configurable, so use
      // neutral defaults (1.0 = no change) for now.
      const uBrightness = gl.getUniformLocation(this.program, 'uBrightness');
      if (uBrightness) {
        gl.uniform1f(uBrightness, 1.0);
      }

      const uContrast = gl.getUniformLocation(this.program, 'uContrast');
      if (uContrast) {
        gl.uniform1f(uContrast, 1.0);
      }

      const uSaturation = gl.getUniformLocation(this.program, 'uSaturation');
      if (uSaturation) {
        gl.uniform1f(uSaturation, 1.0);
      }
    }
  }

  /**
   * Update performance statistics
   */
  private updateStats(startTime: number): void {
    const frameTime = performance.now() - startTime;
    this.lastFrameTime = frameTime;
    
    this.frameCount++;
    const now = performance.now();
    
    if (now - this.fpsUpdateTime >= 1000) {
      this.fps = this.frameCount;
      this.frameCount = 0;
      this.fpsUpdateTime = now;
    }
  }

  /**
   * Get renderer statistics
   */
  getStats(): RendererStats {
    return {
      fps: this.fps,
      frameTime: this.lastFrameTime,
      drawCalls: 1,
    };
  }

  /**
   * Check if WebGL is available
   */
  static isSupported(): boolean {
    const canvas = document.createElement('canvas');
    return !!(canvas.getContext('webgl2') || canvas.getContext('webgl'));
  }

  /**
   * Get WebGL version
   */
  getWebGLVersion(): string {
    if (!this.gl) return 'none';
    return this.gl instanceof WebGL2RenderingContext ? 'WebGL2' : 'WebGL1';
  }

  /**
   * Clean up resources
   */
  dispose(): void {
    const gl = this.gl;
    if (!gl) return;

    // Remove event listeners
    this.canvas.removeEventListener('webglcontextlost', this.handleContextLost);
    this.canvas.removeEventListener('webglcontextrestored', this.handleContextRestored);

    // Delete WebGL resources
    if (this.texture) gl.deleteTexture(this.texture);
    if (this.framebuffer) gl.deleteFramebuffer(this.framebuffer);
    if (this.vertexBuffer) gl.deleteBuffer(this.vertexBuffer);

    this.shaderService.dispose();
    
    this.isInitialized = false;
    this.gl = null;
  }
}