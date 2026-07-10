/**
 * Audio Service for OxideSFC Frontend
 * 
 * Handles Web Audio API initialization, audio buffer management,
 * and playback for SNES emulation audio (16-bit PCM).
 */

export interface AudioServiceConfig {
  sampleRate: number;
  latency: number;
  channels: 'stereo' | 'mono';
}

const DEFAULT_CONFIG: AudioServiceConfig = {
  // The SNES DSP generates samples at ~32kHz. Running the AudioContext at
  // this same rate means the emulator's samples play 1:1 at the correct
  // pitch with no resampling, and (since the render loop generates ~one
  // 32kHz frame's worth of audio per 60fps video frame) the produce/
  // consume rates match, avoiding constant starvation. Using the browser
  // default (often 48kHz) previously both mis-pitched the audio and
  // starved the output by ~33%.
  sampleRate: 32000,
  latency: 50, // milliseconds
  channels: 'stereo',
};

export class AudioService {
  private audioContext: AudioContext | null = null;
  private gainNode: GainNode | null = null;
  private scriptProcessor: ScriptProcessorNode | null = null;
  private isInitialized = false;
  private isPlaying = false;
  private volume = 1.0;
  private isMuted = false;
  private config: AudioServiceConfig;
  // Ring buffers of queued stereo samples (float, -1..1): one flat
  // Float32Array per channel with shared read/write cursors -- the
  // previous array-of-arrays with `slice(1)` per output sample was O(n^2)
  // and thrashed the audio callback. ~1s capacity at 32kHz per channel.
  // `queueAudio` receives interleaved (L,R,L,R,...) samples from the
  // emulator and de-interleaves them into these two rings; mono playback
  // (`channels: 'mono'`) mixes both rings down at output time instead of
  // storing a separate mono representation.
  private ringL = new Float32Array(1 << 15);
  private ringR = new Float32Array(1 << 15);
  private ringRead = 0;
  private ringWrite = 0;
  private ringCount = 0;
  private lastSampleL = 0;
  private lastSampleR = 0;
  /// Playback-rate multiplier, kept in sync with the emulation speed: the
  /// emulator produces speed*32000 samples per wall second, so the output
  /// must consume the ring at the same factor (with linear interpolation
  /// between source samples) or it would drift into overflow/underrun.
  /// This also means pitch and tempo scale with speed, like a real
  /// overclocked console -- which is the point of the speed control.
  private playbackRate = 1.0;
  /// Fractional position between ringRead and ringRead+1 (0..1).
  private readFrac = 0;
  
  // Callback to get audio samples from emulation
  private getAudioSamples: ((count: number) => Promise<number[]>) | null = null;

  constructor(config: Partial<AudioServiceConfig> = {}) {
    this.config = { ...DEFAULT_CONFIG, ...config };
  }

  /**
   * Initialize the Web Audio API context
   */
  async initialize(): Promise<boolean> {
    if (this.isInitialized) {
      console.warn('AudioService already initialized');
      return true;
    }

    try {
      // Create AudioContext at the SNES sample rate so the emulator's
      // samples play at the correct pitch without resampling. Fall back to
      // the browser default if the exact rate isn't supported.
      const AudioContextClass = window.AudioContext || (window as unknown as { webkitAudioContext: typeof window.AudioContext }).webkitAudioContext;
      try {
        this.audioContext = new AudioContextClass({ sampleRate: this.config.sampleRate });
      } catch {
        this.audioContext = new AudioContextClass();
      }
      
      // Resume context if suspended (required for autoplay policies)
      if (this.audioContext.state === 'suspended') {
        await this.audioContext.resume();
      }

      // Create gain node for volume control
      this.gainNode = this.audioContext.createGain();
      this.gainNode.gain.value = this.volume;
      this.gainNode.connect(this.audioContext.destination);

      // Create script processor for low-latency audio
      this.createScriptProcessor();

      this.isInitialized = true;
      console.log('AudioService initialized successfully');
      return true;
    } catch (error) {
      console.error('Failed to initialize AudioService:', error);
      return false;
    }
  }

  /**
   * Create script processor for audio processing
   */
  private createScriptProcessor(): void {
    if (!this.audioContext || !this.gainNode) return;

    // createScriptProcessor only accepts buffer sizes from this fixed set
    // (or 0 for autobuffering) -- passing an arbitrary computed size (e.g.
    // 551 for the 50ms default latency) throws an IndexSizeError.
    const validBufferSizes = [256, 512, 1024, 2048, 4096, 8192, 16384];
    const target = (this.config.sampleRate * (this.config.latency / 1000)) / 4;
    const bufferSize = validBufferSizes.find((size) => size >= target) ?? 16384;

    this.scriptProcessor = this.audioContext.createScriptProcessor(bufferSize, 0, this.config.channels === 'stereo' ? 2 : 1);
    
    // Process audio data
    this.scriptProcessor.onaudioprocess = (event) => {
      this.processAudio(event);
    };

    // Connect to gain node
    this.scriptProcessor.connect(this.gainNode);
  }

  /**
   * Process audio data from the queue
   */
  private processAudio(event: AudioProcessingEvent): void {
    const outputBuffer = event.outputBuffer;
    const channelCount = outputBuffer.numberOfChannels;
    const frameCount = outputBuffer.length;

    const channels: Float32Array[] = [];
    for (let i = 0; i < channelCount; i++) {
      channels.push(outputBuffer.getChannelData(i));
    }

    const ringLen = this.ringL.length;
    for (let frame = 0; frame < frameCount; frame++) {
      let left: number;
      let right: number;
      if (this.ringCount > 1) {
        // Linear interpolation between the current and next queued frame,
        // stepping the read position by playbackRate per output sample so
        // consumption matches the emulator's production rate at any speed.
        const next = (this.ringRead + 1) % ringLen;
        const f = this.readFrac;
        left = this.ringL[this.ringRead] * (1 - f) + this.ringL[next] * f;
        right = this.ringR[this.ringRead] * (1 - f) + this.ringR[next] * f;
        this.readFrac += this.playbackRate;
        while (this.readFrac >= 1 && this.ringCount > 1) {
          this.readFrac -= 1;
          this.ringRead = (this.ringRead + 1) % ringLen;
          this.ringCount--;
        }
        this.lastSampleL = left;
        this.lastSampleR = right;
      } else {
        // Underrun: hold the last sample briefly (decaying to zero) to
        // avoid a hard click, rather than snapping to silence.
        this.lastSampleL *= 0.95;
        this.lastSampleR *= 0.95;
        left = this.lastSampleL;
        right = this.lastSampleR;
      }
      if (channelCount >= 2) {
        // True stereo output: channel 0 = left, channel 1 = right (and
        // any further channels, e.g. a hypothetical multichannel device,
        // just repeat the right channel rather than going silent).
        channels[0][frame] = left;
        for (let channel = 1; channel < channelCount; channel++) {
          channels[channel][frame] = right;
        }
      } else {
        // Mono output device/config: downmix L/R to a single channel
        // rather than dropping one side.
        channels[0][frame] = (left + right) / 2;
      }
    }
  }

  /**
   * Set the callback to fetch audio samples from emulation
   */
  setAudioSource(callback: (count: number) => Promise<number[]>): void {
    this.getAudioSamples = callback;
  }

  /**
   * Queue audio samples for playback
   * @param samples 16-bit signed PCM samples from emulation, interleaved
   * stereo (L0, R0, L1, R1, ...) -- matching what the `get_audio_samples`
   * Tauri command now returns (see `emulationStore.ts`). An odd-length
   * array (which shouldn't happen with a real interleaved buffer) drops
   * its trailing unpaired sample rather than reading past the array.
   */
  queueAudio(samples: number[]): void {
    if (!this.isInitialized || !this.isPlaying) return;
    if (samples.length < 2) return;

    const ringLen = this.ringL.length;
    const frameCount = samples.length >> 1;
    for (let i = 0; i < frameCount; i++) {
      if (this.ringCount >= ringLen) {
        // Overflow: drop the oldest frame to make room (keeps latency
        // bounded rather than growing without limit).
        this.ringRead = (this.ringRead + 1) % ringLen;
        this.ringCount--;
      }
      this.ringL[this.ringWrite] = samples[i * 2] / 32768.0;
      this.ringR[this.ringWrite] = samples[i * 2 + 1] / 32768.0;
      this.ringWrite = (this.ringWrite + 1) % ringLen;
      this.ringCount++;
    }
  }

  /**
   * Start audio playback
   */
  start(): void {
    if (!this.isInitialized) {
      console.warn('AudioService not initialized');
      return;
    }
    this.isPlaying = true;
  }

  /**
   * Stop audio playback
   */
  stop(): void {
    this.isPlaying = false;
    this.ringRead = 0;
    this.ringWrite = 0;
    this.ringCount = 0;
    this.readFrac = 0;
    this.lastSampleL = 0;
    this.lastSampleR = 0;
  }

  /**
   * Set the playback-rate multiplier (kept in sync with the emulation
   * speed by the caller). 1.0 = normal.
   */
  setPlaybackRate(rate: number): void {
    this.playbackRate = Math.max(0.1, Math.min(4, rate));
  }

  /**
   * Get the current playback-rate multiplier.
   */
  getPlaybackRate(): number {
    return this.playbackRate;
  }

  /**
   * Set volume (0-100)
   */
  setVolume(value: number): void {
    this.volume = Math.max(0, Math.min(1, value / 100));
    if (this.gainNode && !this.isMuted) {
      // Smooth transition
      this.gainNode.gain.setTargetAtTime(this.volume, this.audioContext?.currentTime || 0, 0.01);
    }
  }

  /**
   * Get current volume (0-100)
   */
  getVolume(): number {
    return this.volume * 100;
  }

  /**
   * Toggle mute
   */
  setMuted(muted: boolean): void {
    this.isMuted = muted;
    if (this.gainNode) {
      this.gainNode.gain.setTargetAtTime(
        muted ? 0 : this.volume,
        this.audioContext?.currentTime || 0,
        0.01
      );
    }
  }

  /**
   * Check if muted
   */
  getMuted(): boolean {
    return this.isMuted;
  }

  /**
   * Configure latency
   */
  setLatency(latency: number): void {
    this.config.latency = Math.max(10, Math.min(200, latency));
    
    // Recreate script processor with new latency
    if (this.scriptProcessor && this.audioContext) {
      this.scriptProcessor.disconnect();
      this.createScriptProcessor();
    }
  }

  /**
   * Get current latency
   */
  getLatency(): number {
    return this.config.latency;
  }

  /**
   * Configure sample rate
   */
  setSampleRate(sampleRate: number): void {
    this.config.sampleRate = sampleRate;
  }

  /**
   * Get current sample rate
   */
  getSampleRate(): number {
    return this.config.sampleRate;
  }

  /**
   * Configure channels
   */
  setChannels(channels: 'stereo' | 'mono'): void {
    this.config.channels = channels;
  }

  /**
   * Get current channels
   */
  getChannels(): 'stereo' | 'mono' {
    return this.config.channels;
  }

  /**
   * Fetch and queue audio samples (called from render loop)
   */
  async fetchAudio(): Promise<void> {
    if (!this.getAudioSamples || !this.isPlaying) return;

    try {
      // Get 2048 samples per frame (roughly 46ms at 44.1kHz)
      const samples = await this.getAudioSamples(2048);
      if (samples && samples.length > 0) {
        this.queueAudio(samples);
      }
    } catch (error) {
      console.error('Failed to fetch audio samples:', error);
    }
  }

  /**
   * Resume audio context if suspended
   */
  async resume(): Promise<void> {
    if (this.audioContext && this.audioContext.state === 'suspended') {
      await this.audioContext.resume();
    }
  }

  /**
   * Suspend audio context
   */
  async suspend(): Promise<void> {
    if (this.audioContext && this.audioContext.state === 'running') {
      await this.audioContext.suspend();
    }
  }

  /**
   * Dispose of audio resources
   */
  dispose(): void {
    this.stop();
    
    if (this.scriptProcessor) {
      this.scriptProcessor.disconnect();
      this.scriptProcessor = null;
    }

    if (this.gainNode) {
      this.gainNode.disconnect();
      this.gainNode = null;
    }

    if (this.audioContext) {
      this.audioContext.close();
      this.audioContext = null;
    }

    this.isInitialized = false;
    console.log('AudioService disposed');
  }

  /**
   * Check if service is initialized
   */
  isReady(): boolean {
    return this.isInitialized;
  }

  /**
   * Check if audio is playing
   */
  isActive(): boolean {
    return this.isPlaying;
  }
}

// Singleton instance
let audioServiceInstance: AudioService | null = null;

/**
 * Get the singleton AudioService instance
 */
export function getAudioService(config?: Partial<AudioServiceConfig>): AudioService {
  if (!audioServiceInstance) {
    audioServiceInstance = new AudioService(config);
  }
  return audioServiceInstance;
}

/**
 * Reset the singleton instance (useful for testing)
 */
export function resetAudioService(): void {
  if (audioServiceInstance) {
    audioServiceInstance.dispose();
    audioServiceInstance = null;
  }
}
