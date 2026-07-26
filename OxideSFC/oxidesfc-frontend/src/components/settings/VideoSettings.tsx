import { useSettingsStore } from '../../stores/settingsStore';
import { Toggle } from '../common/Toggle';
import { Select } from '../common/Select';
import { SettingsSection, SettingRow, SettingNote } from './SettingsSection';

const RENDERER_OPTIONS = [
  { value: 'webgl', label: 'WebGL' },
  { value: 'webgpu', label: 'WebGPU (not implemented)', disabled: true },
];

/**
 * Upscaling.
 *
 * These are exactly the four values `WebGLRenderer` acts on: 'nearest' and
 * 'bilinear' set the texture filter, 'xbrz' and 'hq2x' additionally select a
 * shader program (see `resolveShaderType`). The list used to offer 'bicubic'
 * and 'lanczos', which the renderer has no branch for -- both silently fell
 * through to bilinear -- while xBRZ and HQ2x, which are fully implemented, were
 * only listed in the *shader* dropdown where nothing reads them. So the two
 * working upscalers were unreachable and two non-existent ones were on offer.
 */
const SCALE_MODE_OPTIONS = [
  { value: 'nearest', label: 'Nearest neighbour' },
  { value: 'bilinear', label: 'Bilinear' },
  { value: 'xbrz', label: 'xBRZ' },
  { value: 'hq2x', label: 'HQ2x' },
];

/**
 * Post-processing. `EmulatorView` translates this into the renderer's single
 * `crtMode` flag, so 'none' and 'crt' are the only values with an effect;
 * 'crt-curved', 'xbrz', 'hq2x' and 'scale2x' were listed here previously and
 * did nothing at all.
 */
const SHADER_OPTIONS = [
  { value: 'none', label: 'None' },
  { value: 'crt', label: 'CRT (scanlines, curvature, vignette)' },
];

const FRAME_LIMIT_OPTIONS = [
  { value: '60', label: '60 fps (console speed)' },
  { value: '120', label: '120 fps' },
  { value: '144', label: '144 fps' },
  { value: '30', label: '30 fps' },
  { value: 'unlimited', label: 'Unlimited' },
];

export function VideoSettings() {
  const { settings, updateSection } = useSettingsStore();
  const video = settings.video;

  const handleChange = (key: string, value: string | boolean | number) =>
    updateSection('video', { [key]: value });

  const crtActive = video.shader === 'crt';
  const upscalerShaderActive = video.scale_mode === 'xbrz' || video.scale_mode === 'hq2x';

  return (
    <div className="space-y-4">
      <SettingsSection
        eyebrow="OUTPUT PIPELINE"
        title="Renderer"
        description="How the console's 256×224 frame is drawn to the window."
      >
        <SettingRow label="Graphics API" help="WebGPU is planned; only the WebGL path exists today.">
          <Select
            options={RENDERER_OPTIONS}
            value={video.renderer}
            onChange={(e) => handleChange('renderer', e.target.value)}
            inputSize="sm"
            className="w-56"
            aria-label="Graphics API"
          />
        </SettingRow>

        <SettingRow
          label="Scale mode"
          help="Nearest keeps tile edges hard. Bilinear softens the upscale, which is closer to how a CRT blended the dithering many games use for transparency."
        >
          <Select
            options={SCALE_MODE_OPTIONS}
            value={video.scale_mode}
            onChange={(e) => handleChange('scale_mode', e.target.value)}
            inputSize="sm"
            className="w-56"
            aria-label="Scale mode"
          />
        </SettingRow>
      </SettingsSection>

      <SettingsSection
        eyebrow="POST-PROCESSING"
        title="Video filter"
        description="Applied after the frame is uploaded, over the whole picture."
      >
        <SettingRow label="Shader">
          <Select
            options={SHADER_OPTIONS}
            value={video.shader}
            onChange={(e) => handleChange('shader', e.target.value)}
            inputSize="sm"
            className="w-56"
            aria-label="Shader"
          />
        </SettingRow>

        {crtActive && upscalerShaderActive && (
          <SettingNote title="CRT is overriding your upscaler" tone="accent">
            The renderer runs one shader program at a time, and CRT wins. Set the
            shader back to None to see {video.scale_mode === 'xbrz' ? 'xBRZ' : 'HQ2x'} again.
          </SettingNote>
        )}
      </SettingsSection>

      <SettingsSection
        eyebrow="PACING"
        title="Frame delivery"
        description="The core is paced by its own master clock; these settings govern how the window presents it."
      >
        <SettingRow
          label="Vertical sync"
          help="Match the display's refresh to avoid tearing."
        >
          <Toggle
            checked={video.vsync}
            onChange={(e) => handleChange('vsync', e.target.checked)}
            aria-label="Vertical sync"
          />
        </SettingRow>

        <SettingRow
          label="Frame limit"
          help="A cap above the console's own rate does not make games run faster; it only affects how often the window redraws."
        >
          <Select
            options={FRAME_LIMIT_OPTIONS}
            value={video.frame_limit}
            onChange={(e) => handleChange('frame_limit', e.target.value)}
            inputSize="sm"
            className="w-56"
            aria-label="Frame limit"
          />
        </SettingRow>

        <SettingNote title="Internal resolution is fixed">
          The core renders at the console's native 256×224 (512×448 in the hi-res
          modes a few games use). There is no internal upscaling to configure --
          everything above happens on the way to the window, so a save state
          taken at one setting looks correct at any other.
        </SettingNote>
      </SettingsSection>
    </div>
  );
}
