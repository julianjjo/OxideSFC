import { useEffect } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';

export interface SettingsProps {
  onRelaunchWizard?: () => void;
}

export function Settings({ onRelaunchWizard }: SettingsProps) {
  const { settings, loadSettings, saveSettings, isLoading } = useSettingsStore();

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  const handleThemeChange = async (theme: string) => {
    await saveSettings({
      ...settings,
      general: { ...settings.general, theme },
    });
  };

  const handleVideoSettingChange = async (key: string, value: unknown) => {
    await saveSettings({
      ...settings,
      video: { ...settings.video, [key]: value },
    });
  };

  const handleAudioSettingChange = async (key: string, value: unknown) => {
    await saveSettings({
      ...settings,
      audio: { ...settings.audio, [key]: value },
    });
  };

  const handleReplayWizard = async () => {
    await saveSettings({
      ...settings,
      general: { ...settings.general, has_completed_onboarding: false },
    });
    onRelaunchWizard?.();
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-lg">Loading settings...</div>
      </div>
    );
  }

  const theme = settings.general.theme;

  return (
    <div className={`h-full overflow-auto ${theme === 'light' ? 'bg-gray-100' : 'bg-slate-900'}`}>
      <div className="max-w-2xl mx-auto p-6 space-y-6">
        <h1 className="text-2xl font-bold mb-6">Settings</h1>

        {/* General Settings */}
        <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
          <h2 className="text-lg font-semibold mb-4">General</h2>
          
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <label>Theme</label>
              <select
                value={settings.general.theme}
                onChange={(e) => handleThemeChange(e.target.value)}
                className={`px-3 py-2 rounded-lg ${
                  theme === 'light'
                    ? 'bg-gray-100 border border-gray-300'
                    : 'bg-slate-700 border border-slate-600'
                }`}
              >
                <option value="dark">Dark</option>
                <option value="light">Light</option>
              </select>
            </div>

            <div className="flex items-center justify-between">
              <label>First-Run Setup Wizard</label>
              <button
                onClick={handleReplayWizard}
                className={`px-3 py-1.5 rounded-lg transition-colors ${
                  theme === 'light'
                    ? 'bg-gray-100 hover:bg-gray-200 border border-gray-300'
                    : 'bg-slate-700 hover:bg-slate-600 border border-slate-600'
                }`}
              >
                Replay Setup Wizard
              </button>
            </div>
          </div>
        </section>

        {/* Video Settings */}
        <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
          <h2 className="text-lg font-semibold mb-4">Video</h2>
          
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <label>VSync</label>
              <input
                type="checkbox"
                checked={settings.video.vsync}
                onChange={(e) => handleVideoSettingChange('vsync', e.target.checked)}
                className="w-5 h-5"
              />
            </div>

            <div className="flex items-center justify-between">
              <label>Frame Limit</label>
              <select
                value={settings.video.frame_limit}
                onChange={(e) => handleVideoSettingChange('frame_limit', e.target.value)}
                className={`px-3 py-2 rounded-lg ${
                  theme === 'light'
                    ? 'bg-gray-100 border border-gray-300'
                    : 'bg-slate-700 border border-slate-600'
                }`}
              >
                <option value="unlimited">Unlimited</option>
                <option value="60">60 FPS</option>
                <option value="120">120 FPS</option>
                <option value="144">144 FPS</option>
              </select>
            </div>

            <div className="flex items-center justify-between">
              <label>Renderer</label>
              <select
                value={settings.video.renderer}
                onChange={(e) => handleVideoSettingChange('renderer', e.target.value)}
                className={`px-3 py-2 rounded-lg ${
                  theme === 'light'
                    ? 'bg-gray-100 border border-gray-300'
                    : 'bg-slate-700 border border-slate-600'
                }`}
              >
                <option value="webgl">WebGL</option>
                <option value="webgpu" disabled>WebGPU (Coming soon)</option>
              </select>
            </div>

            <div className="flex items-center justify-between">
              <label>Shader</label>
              <select
                value={settings.video.shader}
                onChange={(e) => handleVideoSettingChange('shader', e.target.value)}
                className={`px-3 py-2 rounded-lg ${
                  theme === 'light'
                    ? 'bg-gray-100 border border-gray-300'
                    : 'bg-slate-700 border border-slate-600'
                }`}
              >
                <option value="none">None</option>
                <option value="crt">CRT</option>
                <option value="xbrz">xBRZ</option>
              </select>
            </div>
          </div>
        </section>

        {/* Audio Settings */}
        <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
          <h2 className="text-lg font-semibold mb-4">Audio</h2>
          
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <label>Enable Audio</label>
              <input
                type="checkbox"
                checked={settings.audio.enabled}
                onChange={(e) => handleAudioSettingChange('enabled', e.target.checked)}
                className="w-5 h-5"
              />
            </div>

            <div className="flex items-center justify-between">
              <label>Volume</label>
              <input
                type="range"
                min="0"
                max="1"
                step="0.1"
                value={settings.audio.volume}
                onChange={(e) => handleAudioSettingChange('volume', parseFloat(e.target.value))}
                className="w-32"
              />
            </div>

            <div className="flex items-center justify-between">
              <label>Latency (ms)</label>
              <input
                type="number"
                value={settings.audio.latency}
                onChange={(e) => handleAudioSettingChange('latency', parseInt(e.target.value))}
                className={`w-20 px-2 py-1 rounded ${
                  theme === 'light'
                    ? 'bg-gray-100 border border-gray-300'
                    : 'bg-slate-700 border border-slate-600'
                }`}
              />
            </div>
          </div>
        </section>

        {/* Controls */}
        <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
          <h2 className="text-lg font-semibold mb-4">Controls</h2>
          
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <label>Keyboard Input</label>
              <input
                type="checkbox"
                checked={settings.controls.keyboard_enabled}
                onChange={(e) => saveSettings({
                  ...settings,
                  controls: { ...settings.controls, keyboard_enabled: e.target.checked },
                })}
                className="w-5 h-5"
              />
            </div>

            <div className="flex items-center justify-between">
              <label>Gamepad Input</label>
              <input
                type="checkbox"
                checked={settings.controls.gamepad_enabled}
                onChange={(e) => saveSettings({
                  ...settings,
                  controls: { ...settings.controls, gamepad_enabled: e.target.checked },
                })}
                className="w-5 h-5"
              />
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
