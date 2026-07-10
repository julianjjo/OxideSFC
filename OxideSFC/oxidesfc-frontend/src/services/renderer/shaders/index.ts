/**
 * GLSL Shader Sources for OxideSFC Frontend
 * 
 * Contains all vertex and fragment shaders for rendering effects
 * including CRT effects, upscaling shaders, and post-processing.
 */

// ============================================================================
// Vertex Shader - Passthrough
// Simple passthrough shader for full-screen quad rendering
// ============================================================================

export const PASSTHROUGH_VERT = `#version 300 es
precision highp float;

in vec2 aPosition;
in vec2 aTexCoord;

out vec2 vTexCoord;

void main() {
  gl_Position = vec4(aPosition, 0.0, 1.0);
  vTexCoord = aTexCoord;
}
`;

// ============================================================================
// Fragment Shader - CRT Effect
// Implements scanlines, curvature, vignette, and chromatic aberration
// ============================================================================

export const CRT_FRAG = `#version 300 es
precision highp float;

in vec2 vTexCoord;
out vec4 fragColor;

uniform sampler2D uTexture;
uniform vec2 uResolution;
uniform float uTime;

// CRT effect parameters
uniform float uCurvature;
uniform float uScanlineIntensity;
uniform float uVignetteStrength;
uniform float uChromaticAberration;
uniform float uBrightness;
uniform float uContrast;
uniform float uSaturation;

// Apply screen curvature to coordinates
vec2 curveUV(vec2 uv) {
  uv = uv * 2.0 - 1.0;
  vec2 offset = abs(uv.yx) / vec2(uCurvature, uCurvature);
  uv = uv + uv * offset * offset;
  uv = uv * 0.5 + 0.5;
  return uv;
}

// Generate scanline effect
float scanline(vec2 uv) {
  float scanlineCount = uResolution.y * 0.5;
  float scanlinePos = sin(uv.y * scanlineCount * 3.14159 * 2.0);
  return 1.0 - scanlinePos * uScanlineIntensity;
}

// Generate RGB pixel pattern (simulating CRT phosphor arrangement)
vec3 rgbPixelPattern(vec2 uv) {
  float pixelWidth = uResolution.x * 3.0;
  float x = mod(uv.x * pixelWidth, 3.0);
  
  vec3 pattern;
  if (x < 1.0) {
    pattern = vec3(1.0, 0.0, 0.0);
  } else if (x < 2.0) {
    pattern = vec3(0.0, 1.0, 0.0);
  } else {
    pattern = vec3(0.0, 0.0, 1.0);
  }
  
  return pattern * 0.15 + 0.85;
}

// Vignette effect
float vignette(vec2 uv) {
  vec2 center = uv - 0.5;
  float dist = length(center);
  return 1.0 - dist * dist * uVignetteStrength * 2.0;
}

// Chromatic aberration
vec3 chromaticAberration(vec2 uv) {
  float aberration = uChromaticAberration;
  float r = texture(uTexture, uv + vec2(aberration, 0.0)).r;
  float g = texture(uTexture, uv).g;
  float b = texture(uTexture, uv - vec2(aberration, 0.0)).b;
  return vec3(r, g, b);
}

void main() {
  // Apply curvature
  vec2 uv = curveUV(vTexCoord);
  
  // Check if we're outside the curved screen area
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
    fragColor = vec4(0.0, 0.0, 0.0, 1.0);
    return;
  }
  
  // Get base color with chromatic aberration
  vec3 color = chromaticAberration(uv);
  
  // Apply scanlines
  color *= scanline(uv);
  
  // Apply RGB pixel pattern
  color *= rgbPixelPattern(uv);
  
  // Apply vignette
  color *= vignette(uv);
  
  // Apply brightness, contrast, saturation
  color = (color - 0.5) * uContrast + 0.5;
  color *= uBrightness;
  
  // Simple saturation
  float gray = dot(color, vec3(0.299, 0.587, 0.114));
  color = mix(vec3(gray), color, uSaturation);
  
  fragColor = vec4(color, 1.0);
}
`;

// ============================================================================
// Fragment Shader - xBRZ
// xBRZ upscaling algorithm - high quality pixel art upscaling
// ============================================================================

export const XBRZ_FRAG = `#version 300 es
precision highp float;

in vec2 vTexCoord;
out vec4 fragColor;

uniform sampler2D uTexture;
uniform vec2 uResolution;
uniform float uTime;

// xBRZ configuration
const int XBRZ_SCALE = 4;

// Detect if pixel is edge
bool detectEdge(vec3 c1, vec3 c2, float threshold) {
  return distance(c1, c2) > threshold;
}

// Get pixel color at offset
vec3 getPixel(ivec2 coord) {
  return texelFetch(uTexture, coord, 0).rgb;
}

// xBRZ edge detection and blending
vec3 xbrzBlend(vec3 p1, vec3 p2, vec3 p3, vec3 p4, vec3 center, float scale) {
  // Calculate weights based on color differences
  float w1 = 1.0 / (1.0 + distance(p1, center));
  float w2 = 1.0 / (1.0 + distance(p2, center));
  float w3 = 1.0 / (1.0 + distance(p3, center));
  float w4 = 1.0 / (1.0 + distance(p4, center));
  
  float total = w1 + w2 + w3 + w4;
  
  return (p1 * w1 + p2 * w2 + p3 * w3 + p4 * w4) / total;
}

void main() {
  vec2 pixelSize = 1.0 / uResolution;
  ivec2 pixelCoord = ivec2(vTexCoord * uResolution);
  
  vec3 center = getPixel(pixelCoord);
  
  // Get 8 neighboring pixels for edge detection
  vec3 n = getPixel(pixelCoord + ivec2(0, -1));
  vec3 s = getPixel(pixelCoord + ivec2(0, 1));
  vec3 e = getPixel(pixelCoord + ivec2(1, 0));
  vec3 w = getPixel(pixelCoord + ivec2(-1, 0));
  
  // Edge detection thresholds
  float edgeThreshold = 0.1;
  
  bool edgeN = detectEdge(center, n, edgeThreshold);
  bool edgeS = detectEdge(center, s, edgeThreshold);
  bool edgeE = detectEdge(center, e, edgeThreshold);
  bool edgeW = detectEdge(center, w, edgeThreshold);
  
  vec3 result = center;
  
  // Apply edge-aware blending
  if (edgeN || edgeS || edgeE || edgeW) {
    if (edgeN && edgeS) {
      result = (n + s) / 2.0;
    } else if (edgeE && edgeW) {
      result = (e + w) / 2.0;
    } else if (edgeN) {
      result = mix(center, n, 0.5);
    } else if (edgeS) {
      result = mix(center, s, 0.5);
    } else if (edgeE) {
      result = mix(center, e, 0.5);
    } else if (edgeW) {
      result = mix(center, w, 0.5);
    }
  }
  
  fragColor = vec4(result, 1.0);
}
`;

// ============================================================================
// Fragment Shader - HQ2x
// HQ2x upscaling algorithm - smooth upscaling for pixel art
// ============================================================================

export const HQ2X_FRAG = `#version 300 es
precision highp float;

in vec2 vTexCoord;
out vec4 fragColor;

uniform sampler2D uTexture;
uniform vec2 uResolution;
uniform float uTime;

// HQ2x parameters
const float HQ2X_STRENGTH = 0.35;

// Get pixel at offset
vec3 getPixel(ivec2 offset) {
  return texelFetch(uTexture, offset, 0).rgb;
}

// Calculate luma
float luma(vec3 color) {
  return dot(color, vec3(0.299, 0.587, 0.114));
}

// Interpolate based on luma difference
vec3 hq2xInterpolate(vec3 c1, vec3 c2, float diff) {
  float weight = diff / (diff + 1.0) * HQ2X_STRENGTH;
  return mix(c1, c2, weight);
}

void main() {
  ivec2 pixelCoord = ivec2(vTexCoord * uResolution);
  vec3 center = getPixel(pixelCoord);

  // Get neighboring pixels
  vec3 n = getPixel(pixelCoord + ivec2(0, -1));
  vec3 s = getPixel(pixelCoord + ivec2(0, 1));
  vec3 e = getPixel(pixelCoord + ivec2(1, 0));
  vec3 w = getPixel(pixelCoord + ivec2(-1, 0));

  // Get diagonal neighbors
  vec3 ne = getPixel(pixelCoord + ivec2(1, -1));
  vec3 nw = getPixel(pixelCoord + ivec2(-1, -1));
  vec3 se = getPixel(pixelCoord + ivec2(1, 1));
  vec3 sw = getPixel(pixelCoord + ivec2(-1, 1));
  
  // Calculate luma differences
  float lumaCenter = luma(center);
  float lumaN = luma(n);
  float lumaS = luma(s);
  float lumaE = luma(e);
  float lumaW = luma(w);
  
  float diffN = abs(lumaN - lumaCenter);
  float diffS = abs(lumaS - lumaCenter);
  float diffE = abs(lumaE - lumaCenter);
  float diffW = abs(lumaW - lumaCenter);
  
  // Determine interpolation direction
  vec3 result = center;
  
  if (diffN > diffS && diffN > diffE && diffN > diffW) {
    result = hq2xInterpolate(center, n, diffN);
  } else if (diffS > diffE && diffS > diffW) {
    result = hq2xInterpolate(center, s, diffS);
  } else if (diffE > diffW) {
    result = hq2xInterpolate(center, e, diffE);
  } else if (diffW > 0.0) {
    result = hq2xInterpolate(center, w, diffW);
  }
  
  fragColor = vec4(result, 1.0);
}
`;

// ============================================================================
// Fragment Shader - Scale2x
// Scale2x upscaling algorithm - sharp edge-preserving upscaling
// ============================================================================

export const SCALE2X_FRAG = `#version 300 es
precision highp float;

in vec2 vTexCoord;
out vec4 fragColor;

uniform sampler2D uTexture;
uniform vec2 uResolution;
uniform float uTime;

vec3 getPixel(ivec2 offset) {
  return texelFetch(uTexture, offset, 0).rgb;
}

bool isDifferent(vec3 a, vec3 b) {
  return distance(a, b) > 0.1;
}

void main() {
  ivec2 pixelCoord = ivec2(vTexCoord * uResolution);

  vec3 center = getPixel(pixelCoord);
  vec3 n = getPixel(pixelCoord + ivec2(0, -1));
  vec3 s = getPixel(pixelCoord + ivec2(0, 1));
  vec3 e = getPixel(pixelCoord + ivec2(1, 0));
  vec3 w = getPixel(pixelCoord + ivec2(-1, 0));

  vec3 result = center;

  // Scale2x algorithm
  if (isDifferent(n, s) && isDifferent(e, w)) {
    if (!isDifferent(w, n)) {
      result = w;
    }
    if (!isDifferent(n, e)) {
      result = e;
    }
    if (!isDifferent(e, s)) {
      result = e;
    }
    if (!isDifferent(s, w)) {
      result = s;
    }
  }
  
  fragColor = vec4(result, 1.0);
}
`;

// ============================================================================
// Fragment Shader - Post-Process
// Generic post-processing shader for additional effects
// ============================================================================

export const POSTPROCESS_FRAG = `#version 300 es
precision highp float;

in vec2 vTexCoord;
out vec4 fragColor;

uniform sampler2D uTexture;
uniform vec2 uResolution;
uniform float uTime;

// Post-processing parameters
uniform float uBrightness;
uniform float uContrast;
uniform float uSaturation;
uniform float uGamma;
uniform bool uEnableVignette;
uniform float uVignetteRadius;
uniform float uVignetteSoftness;

// Color temperature adjustment
uniform float uColorTemperature; // -1.0 (cool) to 1.0 (warm)

// Noise/grain
uniform float uNoiseAmount;
uniform float uNoiseSpeed;

// Random function for noise
float random(vec2 st) {
  return fract(sin(dot(st.xy, vec2(12.9898, 78.233))) * 43758.5453123);
}

void main() {
  vec4 color = texture(uTexture, vTexCoord);
  
  // Apply brightness
  color.rgb *= uBrightness;
  
  // Apply contrast
  color.rgb = (color.rgb - 0.5) * uContrast + 0.5;
  
  // Apply saturation
  float gray = dot(color.rgb, vec3(0.299, 0.587, 0.114));
  color.rgb = mix(vec3(gray), color.rgb, uSaturation);
  
  // Apply gamma
  color.rgb = pow(color.rgb, vec3(1.0 / uGamma));
  
  // Apply color temperature
  if (uColorTemperature != 0.0) {
    if (uColorTemperature > 0.0) {
      // Warm
      color.r += uColorTemperature * 0.1;
      color.b -= uColorTemperature * 0.1;
    } else {
      // Cool
      color.r += uColorTemperature * 0.1;
      color.b -= uColorTemperature * 0.1;
    }
  }
  
  // Apply vignette
  if (uEnableVignette) {
    vec2 center = vTexCoord - 0.5;
    float dist = length(center);
    float vignette = smoothstep(uVignetteRadius, uVignetteRadius - uVignetteSoftness, dist);
    color.rgb *= vignette;
  }
  
  // Apply noise/grain
  if (uNoiseAmount > 0.0) {
    float noise = random(vTexCoord + uTime * uNoiseSpeed) * 2.0 - 1.0;
    color.rgb += noise * uNoiseAmount;
  }
  
  // Clamp final output
  color.rgb = clamp(color.rgb, 0.0, 1.0);
  
  fragColor = color;
}
`;

// ============================================================================
// Passthrough Fragment (for basic rendering)
// ============================================================================

export const PASSTHROUGH_FRAG = `#version 300 es
precision highp float;

in vec2 vTexCoord;
out vec4 fragColor;

uniform sampler2D uTexture;
uniform vec2 uResolution;

// True passthrough: one texel, no filtering kernel. Earlier versions
// applied a horizontal dither-merge blur here (3-tap, and before that
// 5-tap) to imitate a CRT's bandwidth merging SNES pseudo-transparency
// dithers -- but it visibly softened every sprite and tile edge, and the
// crisp image matters more than merging dither patterns. CRT-style
// smoothing belongs in the dedicated CRT shader, not in the default path.
void main() {
  fragColor = texture(uTexture, vTexCoord);
}
`;