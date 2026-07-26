import { useEffect, useState } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import { Toggle } from '../common/Toggle';
import { Slider } from '../common/Slider';
import { getAudioService, type AudioStats } from '../../services/audio';
import { SettingsSection, SettingRow, SettingBlock, SettingNote } from './SettingsSection';

/** Buffer-target bounds, in ms of queued audio. */
const LATENCY_MIN = 20;
const LATENCY_MAX = 200;

function latencyVerdict(ms: number): string {
  if (ms < 40) return 'Very low — expect crackle if the host stutters';
  if (ms <= 80) return 'Low — the recommended range';
  if (ms <= 140) return 'Safe — audible delay on input';
  return 'High — noticeable delay, only for slow hosts';
}

/** Live readout of the audio pipeline. */
function AudioTelemetry() {
  const [stats, setStats] = useState<AudioStats | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const read = () => {
      const service = getAudioService();
      setReady(service.isReady());
      setStats(service.isReady() ? service.getStats() : null);
    };
    read();
    // The worklet reports upstream about once a second; polling at the same
    // cadence keeps the readout live without doing pointless work.
    const timer = window.setInterval(read, 1000);
    return () => window.clearInterval(timer);
  }, []);

  if (!ready || !stats) {
    return (
      <SettingNote title="No audio device active">
        The pipeline starts with the first game. Load one and come back to see
        buffer fill, underruns and the rate-control correction live.
      </SettingNote>
    );
  }

  const rows: Array<[string, string]> = [
    ['Buffer fill', `${stats.fillMs.toFixed(1)} ms`],
    ['Underruns', String(stats.underrunEvents)],
    ['Dropped frames', String(stats.droppedFrames)],
    // 1.0 means the resampler is not correcting; the worklet nudges this by
    // fractions of a percent to hold the buffer at target without audible
    // pitch drift.
    ['Rate correction', `${((stats.drcRatio - 1) * 100).toFixed(3)} %`],
    ['Device latency', `${(stats.baseLatencyMs + stats.outputLatencyMs).toFixed(1)} ms`],
  ];

  return (
    <div className="mt-3 overflow-hidden rounded-md border border-line">
      {stats.priming && (
        <p className="border-b border-line bg-warn-soft px-3 py-2 text-[0.8125rem] text-warn-text">
          Priming — filling the buffer to target before output starts.
        </p>
      )}
      <dl className="divide-y divide-line">
        {rows.map(([label, value]) => (
          <div key={label} className="flex items-center justify-between px-3 py-2">
            <dt className="text-[0.8125rem] text-mute">{label}</dt>
            <dd className="register text-ink">{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

export function AudioSettings() {
  const { settings, updateSection } = useSettingsStore();
  const audio = settings.audio;

  // Volume and buffer target are held locally while dragging so the slider
  // tracks the pointer at full rate; each change is also applied to the live
  // audio service immediately, and persisted (the store serialises saves, so a
  // drag cannot interleave writes out of order).
  const [volume, setVolume] = useState(Math.round((audio.volume ?? 1) * 100));
  const [latency, setLatency] = useState(audio.latency ?? 60);

  useEffect(() => {
    setVolume(Math.round((audio.volume ?? 1) * 100));
    setLatency(audio.latency ?? 60);
  }, [audio.volume, audio.latency]);

  const patchAudio = (patch: Partial<typeof audio>) => updateSection('audio', patch);

  const handleVolume = (value: number) => {
    setVolume(value);
    getAudioService().setVolume(value);
    void patchAudio({ volume: value / 100 });
  };

  const handleLatency = (value: number) => {
    setLatency(value);
    getAudioService().setLatency(value);
    void patchAudio({ latency: value });
  };

  const handleEnabled = (enabled: boolean) => {
    getAudioService().setMuted(!enabled);
    void patchAudio({ enabled });
  };

  return (
    <div className="space-y-4">
      <SettingsSection
        eyebrow="S-DSP OUTPUT"
        title="Sound"
        action={
          <Toggle
            checked={audio.enabled}
            onChange={(e) => handleEnabled(e.target.checked)}
            aria-label="Enable audio"
          />
        }
        description="The DSP's mixed stereo output, resampled to your device's rate."
      >
        <SettingBlock>
          <Slider
            label="Volume"
            min={0}
            max={100}
            step={1}
            value={volume}
            valueDisplay={(v) => `${v}%`}
            onChange={(e) => handleVolume(parseInt(e.target.value, 10))}
            disabled={!audio.enabled}
          />
        </SettingBlock>

        <SettingNote title="Why there is no separate music and effects volume">
          The S-DSP mixes all eight voices into one stereo pair inside the
          emulated console, exactly as the hardware does. By the time audio
          reaches this app, a jump sound and the background music are the same
          two channels — there is nothing left to balance. This screen used to
          show “SFX volume” and “Music volume” sliders; they saved a number and
          changed nothing, so they are gone.
        </SettingNote>
      </SettingsSection>

      <SettingsSection
        eyebrow="RESAMPLER"
        title="Buffering"
        description="How much audio is kept queued ahead of the device."
      >
        <SettingBlock>
          <Slider
            label="Buffer target"
            min={LATENCY_MIN}
            max={LATENCY_MAX}
            step={5}
            value={latency}
            showMinMax
            valueDisplay={(v) => `${v} ms`}
            onChange={(e) => handleLatency(parseInt(e.target.value, 10))}
            disabled={!audio.enabled}
            helperText={latencyVerdict(latency)}
          />
        </SettingBlock>

        <SettingNote title="What this actually controls">
          The audio worklet holds a ring of decoded samples and continuously
          nudges its resampling ratio — by fractions of a percent, below the
          threshold of audible pitch change — to keep the ring at this target. A
          larger target survives host stutter at the cost of input-to-sound
          delay. This was previously a dropdown of five fixed values that did not
          include the default of 60 ms, so it opened showing nothing selected.
        </SettingNote>
      </SettingsSection>

      <SettingsSection
        eyebrow="DIAGNOSTICS"
        title="Live pipeline"
        description="Read this while a game runs to tell host stutter apart from a buffer set too low."
      >
        <SettingRow label="Source rate" help="The DSP's native output rate; never resampled inside the core.">
          <span className="register text-ink">32 000 Hz</span>
        </SettingRow>
        <AudioTelemetry />
      </SettingsSection>
    </div>
  );
}
