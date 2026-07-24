/**
 * Audio Service for OxideSFC Frontend
 *
 * Plays the emulator's 16-bit PCM through an AudioWorklet: the ring buffer
 * and its interpolating consumer live on the real-time audio thread, so
 * main-thread jank (React renders, GC, long invoke() resolutions) can no
 * longer starve the output the way the old ScriptProcessorNode did (its
 * onaudioprocess ran on the main thread -- one missed deadline = one
 * audible dropout).
 *
 * The worklet also applies dynamic rate control (the snes9x/bsnes/RetroArch
 * technique): the emulation is paced by the Rust side's wall clock while
 * the DAC consumes at its own crystal's rate, and those clocks always
 * disagree by some tens/hundreds of ppm. Without correction the buffer
 * slowly drains (periodic underrun clicks) or fills (unbounded latency).
 * Nudging the resample ratio by at most +/-0.5% -- inaudible -- keeps the
 * fill level pinned to the latency target forever. See
 * plans/audio-handling-research.md for the survey behind this design.
 */

export interface AudioServiceConfig {
  sampleRate: number;
  latency: number;
  channels: 'stereo' | 'mono';
}

/** Playback statistics reported by the worklet (refreshed ~1x/second). */
export interface AudioStats {
  /** Source frames currently queued in the worklet ring. */
  fillFrames: number;
  /** Same fill level in milliseconds of audio. */
  fillMs: number;
  /** Times playback ran out of queued audio (events, not samples). */
  underrunEvents: number;
  /** Source frames discarded because the ring was full. */
  droppedFrames: number;
  /** Current dynamic-rate-control ratio (1.0 = no correction). */
  drcRatio: number;
  /** True while the ring is refilling to the latency target (output silent). */
  priming: boolean;
  /** AudioContext graph latency, ms (0 if unavailable). */
  baseLatencyMs: number;
  /** Context-to-device output latency, ms (0 if unavailable). */
  outputLatencyMs: number;
}

const DEFAULT_CONFIG: AudioServiceConfig = {
  // The SNES DSP generates samples at ~32kHz. Running the AudioContext at
  // this same rate means the emulator's samples play 1:1 at the correct
  // pitch with no resampling. If the browser refuses a 32kHz context the
  // worklet still plays at the right pitch: its base step is
  // sourceRate/contextRate, so a 48kHz fallback context just linearly
  // upsamples.
  sampleRate: 32000,
  // Target fill level of the worklet ring, in ms -- the dynamic rate
  // control setpoint. ~4 video frames: low enough to stay in sync with
  // the picture, high enough that a late requestAnimationFrame or a
  // multi-frame catch-up step (the producer delivers audio in per-step
  // bursts) doesn't starve playback. Mesen2 defaults to 60ms, RetroArch
  // to 64ms; 50ms measurably still underran during bursty stepping.
  latency: 60,
  channels: 'stereo',
};

/**
 * The AudioWorkletProcessor source, inlined and loaded via a Blob URL so it
 * needs no separate asset or bundler configuration (and works identically
 * under `npm run dev` and the production build).
 *
 * Kept as plain, annotation-free JavaScript: this string is executed inside
 * the AudioWorkletGlobalScope, never compiled by TypeScript.
 */
const PROCESSOR_NAME = 'oxidesfc-audio-processor';
const PROCESSOR_SOURCE = `
class OxideSFCAudioProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const opts = (options && options.processorOptions) || {};
    // Ring of de-interleaved source frames. 16384 frames = 512ms at 32kHz:
    // enough headroom for the largest latency target (200ms -> DRC probes
    // up to 2x target) plus a multi-frame catch-up burst from the paced
    // Rust stepper, which can deliver up to 6 video frames (~100ms) at once.
    this.capacity = opts.capacityFrames || 16384;
    this.ringL = new Float32Array(this.capacity);
    this.ringR = new Float32Array(this.capacity);
    this.readIdx = 0;
    this.writeIdx = 0;
    this.count = 0;
    this.readFrac = 0;
    this.lastL = 0;
    this.lastR = 0;
    // Source frames the DRC tries to keep queued (the latency setpoint).
    this.targetFrames = opts.targetFrames || 1600;
    // Max relative rate deviation. +/-0.5% is the value snes9x, bsnes and
    // RetroArch all converged on: inaudible as pitch, larger than any
    // realistic producer/DAC clock skew.
    this.maxDelta = 0.005;
    this.drcRatio = 1;
    // sampleRate is the AudioWorkletGlobalScope global (context rate).
    // The base step makes pitch correct even when the context runs at a
    // different rate than the source (e.g. 32k source on a 48k context).
    this.baseStep = (opts.sourceSampleRate || sampleRate) / sampleRate;
    this.playbackRate = 1;
    this.playing = false;
    this.underrunEvents = 0;
    this.starved = false;
    this.droppedFrames = 0;
    this.framesSinceStats = 0;
    // While priming, output silence and consume nothing until the ring has
    // filled to the latency target. Without this the ring starts (and, after
    // every underrun, restarts) nearly empty and the ONLY thing pushing the
    // fill level up towards the setpoint is the DRC's +/-0.5% differential --
    // at most ~160 source frames per second of headroom, so climbing from one
    // ~533-frame burst to a 1920-frame target took about 12 seconds. Through
    // that whole window the buffer sat near-empty and any main-thread hiccup
    // longer than a frame underran again, so glitches arrived in clusters
    // instead of being absorbed. Priming turns each one into a single bounded
    // pause (~4 video frames) and then a buffer that can actually absorb jitter.
    this.priming = true;
    this.port.onmessage = (event) => this.handleMessage(event.data);
  }

  handleMessage(msg) {
    switch (msg.type) {
      case 'samples':
        this.enqueue(msg.payload);
        break;
      case 'play':
        this.playing = msg.value;
        break;
      case 'rate':
        this.playbackRate = msg.value;
        break;
      case 'target':
        this.targetFrames = msg.value;
        break;
      case 'clear':
        // Drop everything queued (stop / save-state load). lastL/lastR are
        // kept so the output decays from the current level instead of
        // snapping to zero mid-waveform, which would click.
        this.readIdx = 0;
        this.writeIdx = 0;
        this.count = 0;
        this.readFrac = 0;
        // Refill to the target before playing again rather than consuming
        // from an empty ring.
        this.priming = true;
        break;
    }
  }

  // payload: Int16Array of interleaved stereo PCM (L0, R0, L1, R1, ...).
  enqueue(pcm) {
    const frames = pcm.length >> 1;
    const cap = this.capacity;
    for (let i = 0; i < frames; i++) {
      if (this.count >= cap) {
        // Overflow: drop the oldest frame so latency stays bounded. With
        // DRC regulating the fill level this only happens if the producer
        // sustainedly outruns the +/-0.5% correction (e.g. fast-forward
        // beyond what the rate multiplier accounts for).
        this.readIdx = (this.readIdx + 1) % cap;
        this.count--;
        this.droppedFrames++;
      }
      this.ringL[this.writeIdx] = pcm[i * 2] / 32768;
      this.ringR[this.writeIdx] = pcm[i * 2 + 1] / 32768;
      this.writeIdx = (this.writeIdx + 1) % cap;
      this.count++;
    }
  }

  process(inputs, outputs) {
    const out = outputs[0];
    const left = out[0];
    const right = out.length > 1 ? out[1] : null;
    const frames = left.length;
    const cap = this.capacity;

    // Dynamic rate control, recomputed once per 128-frame quantum. fill is
    // 0.5 exactly at the setpoint, so the ratio rests at 1.0 there:
    //   ratio = (1 - d) + 2 * fill * d      (bsnes' formula, d = 0.005)
    // Buffer below target -> consume slower (refills); above -> faster.
    const fill = Math.min(1, this.count / (2 * this.targetFrames));
    this.drcRatio = (1 - this.maxDelta) + 2 * fill * this.maxDelta;
    const step = this.baseStep * this.playbackRate * this.drcRatio;

    // Priming ends once the ring holds a full latency target, so playback
    // resumes with the DRC already at its setpoint (ratio 1.0) instead of
    // pinned to one extreme trying to claw the level up.
    if (this.priming && this.count >= this.targetFrames) {
      this.priming = false;
    }

    for (let i = 0; i < frames; i++) {
      let l;
      let r;
      if (this.playing && !this.priming && this.count > 1) {
        this.starved = false;
        // Linear interpolation between the current and next queued frame,
        // stepping the read position by (baseStep * playbackRate * drc)
        // per output sample.
        const next = (this.readIdx + 1) % cap;
        const f = this.readFrac;
        l = this.ringL[this.readIdx] * (1 - f) + this.ringL[next] * f;
        r = this.ringR[this.readIdx] * (1 - f) + this.ringR[next] * f;
        this.readFrac += step;
        while (this.readFrac >= 1 && this.count > 1) {
          this.readFrac -= 1;
          this.readIdx = (this.readIdx + 1) % cap;
          this.count--;
        }
        this.lastL = l;
        this.lastR = r;
      } else {
        // Underrun (or intentionally stopped/priming): hold the last sample,
        // decaying to zero, to avoid a hard click.
        if (this.playing && !this.priming && !this.starved) {
          this.starved = true;
          this.underrunEvents++;
          // Re-prime: refill to the target before consuming again, so one
          // late frame doesn't leave the ring empty and immediately underrun
          // on the next quantum too.
          this.priming = true;
        }
        this.lastL *= 0.95;
        this.lastR *= 0.95;
        l = this.lastL;
        r = this.lastR;
      }
      if (right) {
        left[i] = l;
        right[i] = r;
        // A hypothetical >2-channel device: repeat the right channel
        // rather than leaving channels silent.
        for (let ch = 2; ch < out.length; ch++) {
          out[ch][i] = r;
        }
      } else {
        // Mono output: downmix instead of dropping a side.
        left[i] = (l + r) / 2;
      }
    }

    this.framesSinceStats += frames;
    if (this.framesSinceStats >= sampleRate) {
      this.framesSinceStats = 0;
      this.port.postMessage({
        type: 'stats',
        fillFrames: this.count,
        underrunEvents: this.underrunEvents,
        droppedFrames: this.droppedFrames,
        drcRatio: this.drcRatio,
        priming: this.priming,
      });
    }
    return true;
  }
}
registerProcessor('${PROCESSOR_NAME}', OxideSFCAudioProcessor);
`;

export class AudioService {
  private audioContext: AudioContext | null = null;
  private gainNode: GainNode | null = null;
  private workletNode: AudioWorkletNode | null = null;
  private workletUrl: string | null = null;
  private isInitialized = false;
  private initPromise: Promise<boolean> | null = null;
  private isPlaying = false;
  private volume = 1.0;
  private isMuted = false;
  private config: AudioServiceConfig;
  private playbackRate = 1.0;
  private lastStats = {
    fillFrames: 0,
    underrunEvents: 0,
    droppedFrames: 0,
    drcRatio: 1,
    priming: true,
  };

  constructor(config: Partial<AudioServiceConfig> = {}) {
    this.config = { ...DEFAULT_CONFIG, ...config };
  }

  /**
   * Initialize the Web Audio API context and the playback worklet.
   *
   * Single-flight: concurrent callers share one in-flight initialization.
   * React 18 StrictMode double-invokes the mounting effect in dev, and two
   * overlapping initialize() calls used to race on the shared context
   * fields across the async addModule() gap -- one call would construct
   * its AudioWorkletNode on the *other* call's context, where the
   * processor module wasn't registered yet ("node name ... is not defined
   * in AudioWorkletGlobalScope").
   */
  async initialize(): Promise<boolean> {
    if (this.isInitialized) {
      return true;
    }
    if (this.initPromise) {
      return this.initPromise;
    }
    this.initPromise = this.doInitialize();
    try {
      return await this.initPromise;
    } finally {
      this.initPromise = null;
    }
  }

  private async doInitialize(): Promise<boolean> {
    try {
      // Create AudioContext at the SNES sample rate so the emulator's
      // samples play 1:1. Fall back to the browser default if the exact
      // rate isn't supported -- the worklet's baseStep keeps the pitch
      // correct either way.
      const AudioContextClass = window.AudioContext || (window as unknown as { webkitAudioContext: typeof window.AudioContext }).webkitAudioContext;
      try {
        this.audioContext = new AudioContextClass({
          sampleRate: this.config.sampleRate,
          latencyHint: 'interactive',
        });
      } catch {
        this.audioContext = new AudioContextClass({ latencyHint: 'interactive' });
      }

      // Resume context if suspended (required for autoplay policies)
      if (this.audioContext.state === 'suspended') {
        await this.audioContext.resume();
      }

      // Create gain node for volume control
      this.gainNode = this.audioContext.createGain();
      this.gainNode.gain.value = this.volume;
      this.gainNode.connect(this.audioContext.destination);

      await this.createWorkletNode();

      this.isInitialized = true;
      if (import.meta.env.DEV) {
        // Dev-only diagnostics hook: lets a devtools console (or an
        // automation session) watch the worklet ring fill, DRC ratio and
        // underrun/drop counters of the live pipeline.
        (window as unknown as Record<string, unknown>).__oxidesfcAudioStats = () => this.getStats();
      }
      console.log('AudioService initialized successfully');
      return true;
    } catch (error) {
      console.error('Failed to initialize AudioService:', error);
      return false;
    }
  }

  /**
   * Load the processor module (from an inline Blob URL) and instantiate
   * the playback node on the audio thread.
   */
  private async createWorkletNode(): Promise<void> {
    // Work with locals captured up front: `this.audioContext` could in
    // principle be swapped by a concurrent initialize/dispose across the
    // `await addModule()` gap, and the node MUST be constructed on the
    // same context the module was registered with.
    const ctx = this.audioContext;
    const gain = this.gainNode;
    if (!ctx || !gain) return;

    this.workletUrl = URL.createObjectURL(
      new Blob([PROCESSOR_SOURCE], { type: 'application/javascript' })
    );
    await ctx.audioWorklet.addModule(this.workletUrl);

    this.workletNode = new AudioWorkletNode(ctx, PROCESSOR_NAME, {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [this.config.channels === 'stereo' ? 2 : 1],
      processorOptions: {
        sourceSampleRate: this.config.sampleRate,
        targetFrames: this.latencyToFrames(this.config.latency),
      },
    });
    this.workletNode.port.onmessage = (event) => {
      if (event.data?.type === 'stats') {
        const { fillFrames, underrunEvents, droppedFrames, drcRatio, priming } = event.data;
        this.lastStats = { fillFrames, underrunEvents, droppedFrames, drcRatio, priming };
      }
    };
    this.workletNode.connect(gain);

    // Re-sync worklet-side state that may have been set before (re)creation.
    this.postToWorklet({ type: 'rate', value: this.playbackRate });
    this.postToWorklet({ type: 'play', value: this.isPlaying });
  }

  private postToWorklet(message: unknown, transfer?: Transferable[]): void {
    this.workletNode?.port.postMessage(message, transfer ?? []);
  }

  private latencyToFrames(latencyMs: number): number {
    return Math.max(1, Math.round((this.config.sampleRate * latencyMs) / 1000));
  }

  /**
   * Queue audio samples for playback.
   * @param samples 16-bit signed PCM from emulation, interleaved stereo
   * (L0, R0, L1, R1, ...) -- an `Int16Array` view over the binary IPC
   * response of the `get_audio_samples` Tauri command (a plain number
   * array is also accepted for convenience/tests). The data is copied
   * before being transferred to the audio thread, so the caller's array
   * remains usable.
   */
  queueAudio(samples: Int16Array | number[]): void {
    if (!this.isInitialized || !this.isPlaying || !this.workletNode) return;
    if (samples.length < 2) return;

    // Copy, then transfer the copy: transferring detaches the underlying
    // ArrayBuffer, and the source may be shared state (e.g. the Zustand
    // store's audioBuffer).
    const pcm = samples instanceof Int16Array ? samples.slice() : Int16Array.from(samples);
    this.postToWorklet({ type: 'samples', payload: pcm }, [pcm.buffer]);
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
    this.postToWorklet({ type: 'play', value: true });
  }

  /**
   * Stop audio playback and discard anything still queued.
   */
  stop(): void {
    this.isPlaying = false;
    this.postToWorklet({ type: 'play', value: false });
    this.postToWorklet({ type: 'clear' });
  }

  /**
   * Discard queued audio without stopping playback. Use when the emulated
   * timeline jumps (save-state load) so stale pre-jump audio isn't played.
   */
  clear(): void {
    this.postToWorklet({ type: 'clear' });
  }

  /**
   * Set the playback-rate multiplier (kept in sync with the emulation
   * speed by the caller). 1.0 = normal. Dynamic rate control multiplies
   * on top of this, so drift correction keeps working at any speed.
   */
  setPlaybackRate(rate: number): void {
    this.playbackRate = Math.max(0.1, Math.min(4, rate));
    this.postToWorklet({ type: 'rate', value: this.playbackRate });
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
   * Configure the latency target (ms) -- the buffer fill level dynamic
   * rate control regulates towards. Applied live; no node recreation.
   */
  setLatency(latency: number): void {
    this.config.latency = Math.max(10, Math.min(200, latency));
    this.postToWorklet({ type: 'target', value: this.latencyToFrames(this.config.latency) });
  }

  /**
   * Get current latency target (ms)
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
   * Configure channels. Takes effect on the next initialize() -- the
   * worklet's output channel count is fixed at node creation.
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
   * Latest playback statistics (worklet ring fill, underruns, drops, DRC
   * ratio; refreshed about once per second) plus context latencies.
   */
  getStats(): AudioStats {
    return {
      ...this.lastStats,
      fillMs: (this.lastStats.fillFrames / this.config.sampleRate) * 1000,
      baseLatencyMs: (this.audioContext?.baseLatency ?? 0) * 1000,
      outputLatencyMs: (this.audioContext?.outputLatency ?? 0) * 1000,
    };
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

    if (this.workletNode) {
      this.workletNode.port.onmessage = null;
      this.workletNode.disconnect();
      this.workletNode = null;
    }

    if (this.workletUrl) {
      URL.revokeObjectURL(this.workletUrl);
      this.workletUrl = null;
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
