import { useSettingsStore } from '../../stores/settingsStore';
import { Toggle } from '../common/Toggle';
import { Select } from '../common/Select';

// Renderer options
const RENDERER_OPTIONS = [
  { value: 'webgl', label: 'WebGL' },
  { value: 'webgpu', label: 'WebGPU (Coming soon)', disabled: true },
];

// Shader options
const SHADER_OPTIONS = [
  { value: 'none', label: 'None' },
  { value: 'crt', label: 'CRT Scanlines' },
  { value: 'crt-curved', label: 'CRT Curved' },
  { value: 'xbrz', label: 'xBRZ' },
  { value: 'hq2x', label: 'HQ2x' },
  { value: 'scale2x', label: 'Scale2x' },
];

// Scale mode options
const SCALE_MODE_OPTIONS = [
  { value: 'nearest', label: 'Nearest Neighbor' },
  { value: 'bilinear', label: 'Bilinear' },
  { value: 'bicubic', label: 'Bicubic' },
  { value: 'lanczos', label: 'Lanczos' },
];

// Frame limit options
const FRAME_LIMIT_OPTIONS = [
  { value: 'unlimited', label: 'Unlimited' },
  { value: '60', label: '60 FPS' },
  { value: '120', label: '120 FPS' },
  { value: '144', label: '144 FPS' },
  { value: '30', label: '30 FPS (Slow)' },
];

export function VideoSettings() {
  const { settings, saveSettings } = useSettingsStore();
  const theme = settings.general.theme;
  const video = settings.video;

  const handleChange = async (key: string, value: string | boolean | number) => {
    await saveSettings({
      ...settings,
      video: {
        ...video,
        [key]: value,
      },
    });
  };

  return (
    <div className="space-y-6">
      {/* Renderer Selection */}
      <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
        <h2 className="text-lg font-semibold mb-4">Renderer</h2>
        
        <div className="space-y-4">
          <Select
            label="Graphics API"
            value={video.renderer}
            options={RENDERER_OPTIONS}
            onChange={(e) => handleChange('renderer', e.target.value)}
            helperText="WebGPU support is planned but not yet implemented"
          />

          <Select
            label="Scale Mode"
            value={video.scale_mode}
            options={SCALE_MODE_OPTIONS}
            onChange={(e) => handleChange('scale_mode', e.target.value)}
            helperText="Determines how the image is scaled when upscaling"
          />
        </div>
      </section>

      {/* Shader Selection */}
      <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
        <h2 className="text-lg font-semibold mb-4">Video Filters</h2>
        
        <div className="space-y-4">
          <Select
            label="Shader"
            value={video.shader}
            options={SHADER_OPTIONS}
            onChange={(e) => handleChange('shader', e.target.value)}
            helperText="Apply post-processing effects to the output"
          />

          {video.shader !== 'none' && (
            <div className={`p-3 rounded-lg text-sm ${
              theme === 'light' ? 'bg-blue-50 text-blue-700' : 'bg-blue-900/30 text-blue-300'
            }`}>
              Shader preview will appear in the emulator view when a game is running.
            </div>
          )}
        </div>
      </section>

      {/* VSync & Frame Limit */}
      <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
        <h2 className="text-lg font-semibold mb-4">Performance</h2>
        
        <div className="space-y-4">
          <Toggle
            checked={video.vsync}
            onChange={(e) => handleChange('vsync', e.target.checked)}
            label="Vertical Sync (VSync)"
            description="Synchronize display refresh rate with frame rate to prevent tearing"
          />

          <Select
            label="Frame Limit"
            value={video.frame_limit}
            options={FRAME_LIMIT_OPTIONS}
            onChange={(e) => handleChange('frame_limit', e.target.value)}
            helperText="Limit the maximum frames per second"
          />
        </div>
      </section>

      {/* Advanced Settings */}
      <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
        <h2 className="text-lg font-semibold mb-4">Advanced</h2>
        
        <div className="space-y-4">
          <div className={`p-3 rounded-lg text-sm ${
            theme === 'light' ? 'bg-gray-100 text-gray-600' : 'bg-slate-700 text-slate-400'
          }`}>
            <div className="flex items-center gap-2 mb-2">
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              <span className="font-medium">Resolution Scaling</span>
            </div>
            <p>Internal resolution is fixed at 256x224 (SNES native). Output scaling can be adjusted in the emulator view.</p>
          </div>
        </div>
      </section>
    </div>
  );
}