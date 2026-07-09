/**
 * Welcome Wizard Component
 * 
 * First-time setup wizard with the following steps:
 * 1. Welcome
 * 2. Language selection
 * 3. Initial ROM folder selection
 * 4. Controller setup (keyboard or gamepad)
 * 5. Controller profile creation
 * 6. Basic video/audio settings
 * 7. Metadata fetch settings
 * 8. Complete and scan library
 */

import { useState, useEffect } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import { Button } from '../common/Button';
import { Toggle } from '../common/Toggle';
import { Slider } from '../common/Slider';
import { Select } from '../common/Select';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

import type { WizardStep, WizardFormData } from './types';
import {
  DEFAULT_WIZARD_FORM_DATA,
  LANGUAGE_OPTIONS,
  WIZARD_STEPS,
} from './types';
import {
  DEFAULT_KEYBOARD_MAPPING,
  DEFAULT_GAMEPAD_MAPPING,
} from '../../services/controller';

// ============================================================================
// Wizard Component
// ============================================================================

export interface WelcomeWizardProps {
  isOpen: boolean;
  onComplete: () => void;
  onClose: () => void;
  isRerun?: boolean;
}

export function WelcomeWizard({ isOpen, onComplete, isRerun = false }: WelcomeWizardProps) {
  const { settings, saveSettings } = useSettingsStore();
  const theme = settings.general.theme;

  const [currentStep, setCurrentStep] = useState<WizardStep>('welcome');
  const [formData, setFormData] = useState<WizardFormData>(DEFAULT_WIZARD_FORM_DATA);
  const [isScanning, setIsScanning] = useState(false);
  const [mappingKey, setMappingKey] = useState<string | null>(null);
  const [isListening, setIsListening] = useState(false);
  const [gamepads, setGamepads] = useState<(Gamepad | null)[]>([]);

  // Poll for gamepads
  useEffect(() => {
    if (currentStep !== 'controller-type' && currentStep !== 'controller-profile') return;

    const updateGamepads = () => {
      const gp = navigator.getGamepads();
      setGamepads(Array.from(gp));
    };

    updateGamepads();
    const interval = setInterval(updateGamepads, 500);
    return () => clearInterval(interval);
  }, [currentStep]);

  // Keyboard mapping listener
  useEffect(() => {
    if (!isListening || !mappingKey) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      
      if (e.code === 'Escape') {
        setIsListening(false);
        setMappingKey(null);
        return;
      }

      const newMapping = { ...formData.controllerProfile.buttonMapping };
      newMapping[mappingKey as keyof typeof newMapping] = e.code;
      
      setFormData({
        ...formData,
        controllerProfile: {
          ...formData.controllerProfile,
          buttonMapping: newMapping,
        },
      });
      
      setIsListening(false);
      setMappingKey(null);
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isListening, mappingKey, formData]);

  const stepConfig = WIZARD_STEPS.find(s => s.id === currentStep);
  const currentStepIndex = WIZARD_STEPS.findIndex(s => s.id === currentStep);

  const nextStep = () => {
    const nextIndex = currentStepIndex + 1;
    if (nextIndex < WIZARD_STEPS.length) {
      setCurrentStep(WIZARD_STEPS[nextIndex].id);
    }
  };

  const prevStep = () => {
    const prevIndex = currentStepIndex - 1;
    if (prevIndex >= 0) {
      setCurrentStep(WIZARD_STEPS[prevIndex].id);
    }
  };

  const handleSkip = () => {
    nextStep();
  };

  const handleComplete = async () => {
    try {
      // Build the ROM folder / metadata changes to the library settings
      // together, since both target the same nested `library` object.
      const libraryChanges: typeof settings.library = {
        ...settings.library,
        use_metadata: formData.metadataSettings.enabled,
      };
      if (formData.romFolder) {
        libraryChanges.folders = [formData.romFolder];
        libraryChanges.scan_recursive = formData.scanSubfolders;
      }

      const controlsChanges: typeof settings.controls =
        formData.controllerType === 'keyboard'
          ? {
              ...settings.controls,
              keyboard_enabled: true,
              gamepad_enabled: false,
              keyboard_mapping: formData.controllerProfile.buttonMapping as unknown as Record<string, string>,
            }
          : {
              ...settings.controls,
              keyboard_enabled: false,
              gamepad_enabled: true,
            };

      // Merge every wizard step's collected values into a single settings
      // object and save it in one call. saveSettings replaces the whole
      // settings object rather than merging, so building several
      // sequential calls (each spread from the same stale `settings`)
      // would clobber all but the last call's changes.
      await saveSettings({
        ...settings,
        general: {
          ...settings.general,
          language: formData.language,
        },
        library: libraryChanges,
        audio: {
          ...settings.audio,
          enabled: formData.audioSettings.enabled,
          volume: formData.audioSettings.volume,
        },
        video: {
          ...settings.video,
          vsync: formData.videoSettings.vsync,
          renderer: formData.videoSettings.renderer,
          shader: formData.videoSettings.shader,
          scale_mode: formData.videoSettings.scaleMode,
        },
        controls: controlsChanges,
      });

      // Mark wizard as complete in localStorage
      localStorage.setItem('wizard_complete', 'true');
      localStorage.setItem('wizard_version', '1');

      // Scan library if folder was selected
      if (formData.romFolder && formData.romFolder.trim() !== '') {
        setIsScanning(true);
        try {
          await invoke('add_game_folder', { path: formData.romFolder });
        } catch (error) {
          console.error('Failed to scan library:', error);
        }
        setIsScanning(false);
      }

      onComplete();
    } catch (error) {
      console.error('Failed to save wizard settings:', error);
    }
  };

  const handleSelectFolder = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select ROM Folder',
      });
      if (selected && typeof selected === 'string') {
        setFormData({ ...formData, romFolder: selected });
      }
    } catch (error) {
      console.error('Failed to select folder:', error);
    }
  };

  const handleRemapKey = (key: string) => {
    setMappingKey(key);
    setIsListening(true);
  };

  const SNES_BUTTONS = [
    { key: 'up', label: 'Up' },
    { key: 'down', label: 'Down' },
    { key: 'left', label: 'Left' },
    { key: 'right', label: 'Right' },
    { key: 'a', label: 'A' },
    { key: 'b', label: 'B' },
    { key: 'x', label: 'X' },
    { key: 'y', label: 'Y' },
    { key: 'l', label: 'L' },
    { key: 'r', label: 'R' },
    { key: 'start', label: 'Start' },
    { key: 'select', label: 'Select' },
  ];

  const getKeyLabel = (action: string): string => {
    const mapping = formData.controllerProfile.buttonMapping;
    const entry = Object.entries(mapping).find(([, value]) => value === action);
    if (!entry) return 'Not bound';
    const keyCode = entry[0];
    return keyCode.replace('Key', '').replace('Digit', '').replace('Arrow', '');
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/70">
      <div className={`w-full max-w-2xl rounded-xl shadow-2xl ${
        theme === 'light' ? 'bg-white' : 'bg-slate-800'
      }`}>
        {/* Progress bar */}
        <div className="h-1 bg-slate-700 rounded-t-xl overflow-hidden">
          <div 
            className="h-full bg-blue-600 transition-all duration-300"
            style={{ width: `${((currentStepIndex + 1) / WIZARD_STEPS.length) * 100}%` }}
          />
        </div>

        {/* Header */}
        <div className="p-6 border-b border-slate-700">
          <h2 className={`text-2xl font-bold ${
            theme === 'light' ? 'text-gray-900' : 'text-white'
          }`}>
            {stepConfig?.title}
          </h2>
          <p className={`mt-1 ${
            theme === 'light' ? 'text-gray-600' : 'text-slate-400'
          }`}>
            {stepConfig?.description}
          </p>
        </div>

        {/* Content */}
        <div className="p-6 min-h-[300px]">
          {currentStep === 'welcome' && (
            <div className="text-center py-8">
              <div className="text-6xl mb-4">🎮</div>
              <h3 className={`text-xl font-semibold mb-4 ${
                theme === 'light' ? 'text-gray-900' : 'text-white'
              }`}>
                Welcome to OxideSFC
              </h3>
              <p className={`max-w-md mx-auto ${
                theme === 'light' ? 'text-gray-600' : 'text-slate-400'
              }`}>
                {isRerun 
                  ? 'Run the setup wizard again to configure your preferences.'
                  : 'Let\'s get you set up with the best SNES emulation experience. This wizard will guide you through the initial configuration.'
                }
              </p>
            </div>
          )}

          {currentStep === 'language' && (
            <div className="space-y-4">
              <p className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>
                Select your preferred language for the interface.
              </p>
              <Select
                label="Language"
                value={formData.language}
                onChange={(e) => setFormData({ ...formData, language: e.target.value })}
                options={LANGUAGE_OPTIONS.map(lang => ({
                  value: lang.code,
                  label: `${lang.nativeName} (${lang.name})`,
                }))}
              />
            </div>
          )}

          {currentStep === 'rom-folder' && (
            <div className="space-y-4">
              <p className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>
                Choose the folder where your SNES ROM files are located. You can always add more folders later in settings.
              </p>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={formData.romFolder}
                  readOnly
                  placeholder="No folder selected"
                  className={`flex-1 px-3 py-2 rounded-lg ${
                    theme === 'light'
                      ? 'bg-gray-100 border border-gray-300'
                      : 'bg-slate-700 border border-slate-600'
                  }`}
                />
                <Button onClick={handleSelectFolder}>Browse</Button>
              </div>
              <Toggle
                label="Scan subfolders"
                checked={formData.scanSubfolders}
                onChange={(e) => setFormData({ ...formData, scanSubfolders: e.target.checked })}
              />
            </div>
          )}

          {currentStep === 'controller-type' && (
            <div className="space-y-4">
              <p className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>
                What type of controller will you use to play games?
              </p>
              <div className="grid grid-cols-2 gap-4">
                <button
                  onClick={() => {
                    setFormData({
                      ...formData,
                      controllerType: 'keyboard',
                      controllerProfile: {
                        ...formData.controllerProfile,
                        type: 'keyboard',
                        buttonMapping: DEFAULT_KEYBOARD_MAPPING,
                      },
                    });
                    nextStep();
                  }}
                  className={`p-6 rounded-lg border-2 transition-colors ${
                    formData.controllerType === 'keyboard'
                      ? 'border-blue-500 bg-blue-500/10'
                      : theme === 'light'
                        ? 'border-gray-200 hover:border-gray-300'
                        : 'border-slate-600 hover:border-slate-500'
                  }`}
                >
                  <div className="text-4xl mb-2">⌨️</div>
                  <div className={`font-semibold ${
                    theme === 'light' ? 'text-gray-900' : 'text-white'
                  }`}>Keyboard</div>
                </button>
                <button
                  onClick={() => {
                    setFormData({
                      ...formData,
                      controllerType: 'gamepad',
                      controllerProfile: {
                        ...formData.controllerProfile,
                        type: 'gamepad',
                        buttonMapping: DEFAULT_GAMEPAD_MAPPING,
                      },
                    });
                    nextStep();
                  }}
                  className={`p-6 rounded-lg border-2 transition-colors ${
                    formData.controllerType === 'gamepad'
                      ? 'border-blue-500 bg-blue-500/10'
                      : theme === 'light'
                        ? 'border-gray-200 hover:border-gray-300'
                        : 'border-slate-600 hover:border-slate-500'
                  }`}
                >
                  <div className="text-4xl mb-2">🎮</div>
                  <div className={`font-semibold ${
                    theme === 'light' ? 'text-gray-900' : 'text-white'
                  }`}>Gamepad</div>
                  {gamepads.length > 0 && (
                    <div className="text-sm text-green-500 mt-1">
                      {gamepads.length} connected
                    </div>
                  )}
                </button>
              </div>
            </div>
          )}

          {currentStep === 'controller-profile' && (
            <div className="space-y-4">
              {formData.controllerType === 'keyboard' ? (
                <>
                  <p className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>
                    Configure your keyboard mappings for playing games.
                  </p>
                  {isListening && (
                    <div className={`p-3 rounded-lg text-center ${
                      theme === 'light' ? 'bg-blue-50 text-blue-700' : 'bg-blue-900/30 text-blue-300'
                    }`}>
                      Press any key to map to "{mappingKey}" (ESC to cancel)
                    </div>
                  )}
                  <div className="grid grid-cols-3 gap-2">
                    {SNES_BUTTONS.map((btn) => (
                      <div key={btn.key} className="flex items-center justify-between p-2 rounded bg-slate-700">
                        <span className="text-sm">{btn.label}</span>
                        <button
                          onClick={() => handleRemapKey(btn.key)}
                          className="px-2 py-1 text-xs bg-slate-600 rounded hover:bg-slate-500"
                        >
                          {isListening && mappingKey === btn.key ? '...' : 
                            getKeyLabel(formData.controllerProfile.buttonMapping[btn.key as keyof typeof formData.controllerProfile.buttonMapping])}
                        </button>
                      </div>
                    ))}
                  </div>
                </>
              ) : (
                <>
                  <p className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>
                    Gamepad mappings use sensible defaults and will be auto-detected when you play.
                  </p>
                  <div className={`p-4 rounded-lg ${
                    theme === 'light' ? 'bg-gray-100' : 'bg-slate-700'
                  }`}>
                    {gamepads.filter(Boolean).length > 0 ? (
                      <div className="flex items-center gap-2 text-green-500 text-sm font-medium">
                        <span>✓</span>
                        <span>
                          {gamepads.filter(Boolean).length} gamepad{gamepads.filter(Boolean).length > 1 ? 's' : ''} detected
                        </span>
                      </div>
                    ) : (
                      <div className={`text-sm ${theme === 'light' ? 'text-gray-600' : 'text-slate-400'}`}>
                        No gamepad detected yet. Connect one and press a button, or continue &mdash; it'll be picked up automatically.
                      </div>
                    )}
                  </div>
                  <p className={`text-sm ${theme === 'light' ? 'text-gray-500' : 'text-slate-500'}`}>
                    You can fine-tune button mapping and choose a controller profile later in Settings &rarr; Controls.
                  </p>
                </>
              )}
            </div>
          )}

          {currentStep === 'audio-video' && (
            <div className="space-y-6">
              <div>
                <h4 className={`font-semibold mb-4 ${
                  theme === 'light' ? 'text-gray-900' : 'text-white'
                }`}>Video Settings</h4>
                <Toggle
                  label="VSync"
                  description="Synchronize frame rate with display"
                  checked={formData.videoSettings.vsync}
                  onChange={(e) => setFormData({
                    ...formData,
                    videoSettings: { ...formData.videoSettings, vsync: e.target.checked },
                  })}
                />
                <div className="mt-4">
                  <Select
                    label="Renderer"
                    value={formData.videoSettings.renderer}
                    onChange={(e) => setFormData({
                      ...formData,
                      videoSettings: { ...formData.videoSettings, renderer: e.target.value },
                    })}
                    options={[
                      { value: 'webgl', label: 'WebGL' },
                      { value: 'canvas', label: 'Canvas (CPU)' },
                    ]}
                  />
                </div>
                <div className="mt-4">
                  <Select
                    label="Scale Mode"
                    value={formData.videoSettings.scaleMode}
                    onChange={(e) => setFormData({
                      ...formData,
                      videoSettings: { ...formData.videoSettings, scaleMode: e.target.value },
                    })}
                    options={[
                      { value: 'nearest', label: 'Nearest Neighbor' },
                      { value: 'bilinear', label: 'Bilinear' },
                    ]}
                  />
                </div>
              </div>
              <div>
                <h4 className={`font-semibold mb-4 ${
                  theme === 'light' ? 'text-gray-900' : 'text-white'
                }`}>Audio Settings</h4>
                <Toggle
                  label="Enable Audio"
                  checked={formData.audioSettings.enabled}
                  onChange={(e) => setFormData({
                    ...formData,
                    audioSettings: { ...formData.audioSettings, enabled: e.target.checked },
                  })}
                />
                {formData.audioSettings.enabled && (
                  <div className="mt-4">
                    <Slider
                      label="Volume"
                      value={formData.audioSettings.volume}
                      min={0}
                      max={1}
                      step={0.1}
                      showValue
                      valueDisplay={(v) => `${Math.round(v * 100)}%`}
                      onChange={(e) => setFormData({
                        ...formData,
                        audioSettings: { ...formData.audioSettings, volume: parseFloat(e.target.value) },
                      })}
                    />
                  </div>
                )}
              </div>
            </div>
          )}

          {currentStep === 'metadata' && (
            <div className="space-y-4">
              <p className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>
                OxideSFC can fetch game information like box art, descriptions, and release dates from online databases.
              </p>
              <Toggle
                label="Fetch metadata"
                description="Automatically download game information"
                checked={formData.metadataSettings.enabled}
                onChange={(e) => setFormData({
                  ...formData,
                  metadataSettings: { ...formData.metadataSettings, enabled: e.target.checked },
                })}
              />
              {formData.metadataSettings.enabled && (
                <div className="mt-4">
                  <Select
                    label="Preferred Source"
                    value={formData.metadataSettings.preferredSource}
                    onChange={(e) => setFormData({
                      ...formData,
                      metadataSettings: { ...formData.metadataSettings, preferredSource: e.target.value },
                    })}
                    options={[
                      { value: 'screenscraper', label: 'ScreenScraper' },
                      { value: 'igdb', label: 'IGDB' },
                    ]}
                  />
                </div>
              )}
            </div>
          )}

          {currentStep === 'complete' && (
            <div className="text-center py-8">
              <div className="text-6xl mb-4">🎉</div>
              <h3 className={`text-xl font-semibold mb-4 ${
                theme === 'light' ? 'text-gray-900' : 'text-white'
              }`}>
                You're All Set!
              </h3>
              <p className={`max-w-md mx-auto ${
                theme === 'light' ? 'text-gray-600' : 'text-slate-400'
              }`}>
                {formData.romFolder 
                  ? 'Your library is being scanned. Enjoy your games!'
                  : 'You can add ROM folders later from settings. Enjoy your games!'
                }
              </p>
              {isScanning && (
                <div className="mt-4">
                  <div className="w-full h-2 bg-slate-700 rounded-full overflow-hidden">
                    <div className="h-full bg-blue-600 animate-pulse" style={{ width: '100%' }} />
                  </div>
                  <p className="text-sm mt-2">Scanning library...</p>
                </div>
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="p-6 border-t border-slate-700 flex justify-between">
          <div>
            {currentStepIndex > 0 && currentStep !== 'welcome' && currentStep !== 'complete' && (
              <Button variant="ghost" onClick={prevStep}>
                Back
              </Button>
            )}
          </div>
          <div className="flex gap-2">
            {currentStep !== 'welcome' && currentStep !== 'complete' && stepConfig?.canSkip && (
              <Button variant="ghost" onClick={handleSkip}>
                Skip
              </Button>
            )}
            {currentStep === 'complete' ? (
              <Button onClick={handleComplete} disabled={isScanning}>
                {isScanning ? 'Finishing...' : 'Finish'}
              </Button>
            ) : currentStep !== 'controller-type' ? (
              <Button onClick={nextStep}>Next</Button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
