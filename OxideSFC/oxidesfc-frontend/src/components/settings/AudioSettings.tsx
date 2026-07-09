import { useState, useEffect } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import { Toggle } from '../common/Toggle';
import { Slider } from '../common/Slider';
import { Select } from '../common/Select';
import { getAudioService } from '../../services/audio';

// Latency options
const LATENCY_OPTIONS = [
  { value: '20', label: '20ms (Ultra Low)' },
  { value: '50', label: '50ms (Low)' },
  { value: '100', label: '100ms (Medium)' },
  { value: '150', label: '150ms (High)' },
  { value: '200', label: '200ms (Very High)' },
];

export function AudioSettings() {
  const { settings, saveSettings } = useSettingsStore();
  const theme = (settings.general.theme as 'dark' | 'light') || 'dark';
  const audio = settings.audio;

  // Local state for volume sliders (normalized to 0-100)
  const [masterVolume, setMasterVolume] = useState(Math.round((audio.volume || 1) * 100));
  const [sfxVolume, setSfxVolume] = useState(audio.sfx_volume ?? 100);
  const [musicVolume, setMusicVolume] = useState(audio.music_volume ?? 100);
  const [latency, setLatency] = useState(audio.latency || 50);
  const [enableBuffering, setEnableBuffering] = useState(audio.buffering_enabled ?? true);

  // Update local state when settings load
  useEffect(() => {
    if (audio.volume !== undefined) {
      setMasterVolume(Math.round(audio.volume * 100));
    }
    if (audio.latency !== undefined) {
      setLatency(audio.latency);
    }
    if (audio.sfx_volume !== undefined) {
      setSfxVolume(audio.sfx_volume);
    }
    if (audio.music_volume !== undefined) {
      setMusicVolume(audio.music_volume);
    }
    if (audio.buffering_enabled !== undefined) {
      setEnableBuffering(audio.buffering_enabled);
    }
  }, [audio.volume, audio.latency, audio.sfx_volume, audio.music_volume, audio.buffering_enabled]);

  const handleMasterVolumeChange = async (value: number) => {
    setMasterVolume(value);
    const normalizedVolume = value / 100;

    // Apply live so the running game's audio updates immediately, instead
    // of waiting for the settings save round-trip through Tauri.
    getAudioService().setVolume(value);

    await saveSettings({
      ...settings,
      audio: {
        ...audio,
        volume: normalizedVolume,
      },
    });
  };

  const handleSfxVolumeChange = async (value: number) => {
    setSfxVolume(value);
    await saveSettings({
      ...settings,
      audio: {
        ...audio,
        sfx_volume: value,
      },
    });
  };

  const handleMusicVolumeChange = async (value: number) => {
    setMusicVolume(value);
    await saveSettings({
      ...settings,
      audio: {
        ...audio,
        music_volume: value,
      },
    });
  };

  const handleLatencyChange = async (value: number) => {
    setLatency(value);

    // Apply live so the audio service reconfigures its buffer immediately.
    getAudioService().setLatency(value);

    await saveSettings({
      ...settings,
      audio: {
        ...audio,
        latency: value,
      },
    });
  };

  const handleToggleAudio = async (enabled: boolean) => {
    // Apply live: muting/unmuting should take effect on the currently
    // playing game immediately, not just on the next settings load.
    getAudioService().setMuted(!enabled);

    await saveSettings({
      ...settings,
      audio: {
        ...audio,
        enabled,
      },
    });
  };

  const handleToggleBuffering = async (enabled: boolean) => {
    setEnableBuffering(enabled);
    await saveSettings({
      ...settings,
      audio: {
        ...audio,
        buffering_enabled: enabled,
      },
    });
  };

  const containerClass = theme === 'light'
    ? 'bg-white'
    : 'bg-slate-800';

  return (
    <div className="space-y-6">
      {/* Main Audio Toggle */}
      <section className={`rounded-lg p-6 ${containerClass}`}>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">Audio Output</h2>
          <Toggle
            checked={audio.enabled}
            onChange={(e) => handleToggleAudio(e.target.checked)}
            label="Enabled"
          />
        </div>
      </section>

      {/* Volume Settings */}
      <section className={`rounded-lg p-6 ${containerClass}`}>
        <h2 className="text-lg font-semibold mb-4">Volume</h2>
        
        <div className="space-y-6">
          {/* Master Volume */}
          <Slider
            label="Master Volume"
            value={masterVolume}
            min={0}
            max={100}
            step={1}
            showValue
            valueDisplay={(v) => `${v}%`}
            onChange={(e) => handleMasterVolumeChange(parseInt(e.target.value, 10))}
            disabled={!audio.enabled}
          />

          {/* SFX Volume */}
          <Slider
            label="SFX Volume"
            value={sfxVolume}
            min={0}
            max={100}
            step={1}
            showValue
            valueDisplay={(v) => `${v}%`}
            onChange={(e) => handleSfxVolumeChange(parseInt(e.target.value, 10))}
            disabled={!audio.enabled}
            helperText="Sound effects volume relative to master"
          />

          {/* Music Volume */}
          <Slider
            label="Music Volume"
            value={musicVolume}
            min={0}
            max={100}
            step={1}
            showValue
            valueDisplay={(v) => `${v}%`}
            onChange={(e) => handleMusicVolumeChange(parseInt(e.target.value, 10))}
            disabled={!audio.enabled}
            helperText="Background music volume relative to master"
          />
        </div>
      </section>

      {/* Latency Settings */}
      <section className={`rounded-lg p-6 ${containerClass}`}>
        <h2 className="text-lg font-semibold mb-4">Latency & Performance</h2>
        
        <div className="space-y-4">
          <Select
            label="Audio Latency"
            value={String(latency)}
            options={LATENCY_OPTIONS}
            onChange={(e) => handleLatencyChange(parseInt(e.target.value, 10))}
            helperText="Lower latency reduces audio delay but may cause audio glitches"
            disabled={!audio.enabled}
          />

          <Toggle
            checked={enableBuffering}
            onChange={(e) => handleToggleBuffering(e.target.checked)}
            label="Enable Audio Buffering"
            description="Use buffered audio output for smoother playback"
            disabled={!audio.enabled}
          />
        </div>
      </section>

      {/* Audio Information */}
      <section className={`rounded-lg p-6 ${containerClass}`}>
        <h2 className="text-lg font-semibold mb-4">Audio Information</h2>
        
        <div className={`p-3 rounded-lg text-sm ${
          theme === 'light' ? 'bg-gray-100 text-gray-600' : 'bg-slate-700 text-slate-400'
        }`}>
          <div className="flex items-center gap-2 mb-2">
            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <span className="font-medium">SNES Audio</span>
          </div>
          <p>SNES audio output is 8-bit PCM at 32kHz. The emulator upscales to higher sample rates for better quality on modern audio hardware.</p>
        </div>
      </section>
    </div>
  );
}
