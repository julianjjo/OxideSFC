/**
 * First-run setup wizard.
 *
 * Steps: welcome, language, ROM folder, controller type, button mapping,
 * audio/video, metadata, finish (which also scans the chosen folder).
 */

import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useSettingsStore } from '../../stores/settingsStore';
import { Button } from '../common/Button';
import { Toggle } from '../common/Toggle';
import { Slider } from '../common/Slider';
import { Select } from '../common/Select';
import { Input } from '../common/Input';
import { MarkSFC, IconFolder, IconGamepad, IconCheck, IconClose } from '../common/icons';
import { formatKeyCode } from '../../domain/keyLabel';

import type { WizardStep, WizardFormData } from './types';
import { DEFAULT_WIZARD_FORM_DATA, LANGUAGE_OPTIONS, WIZARD_STEPS } from './types';
import {
  DEFAULT_KEYBOARD_MAPPING,
  DEFAULT_GAMEPAD_MAPPING,
} from '../../services/controller';
import type { ButtonMapping } from '../../services/controller';

/**
 * The twelve buttons a Super Famicom pad has, derived from `ButtonMapping` so
 * the list here cannot drift out of step with the type. Using the key union
 * instead of `string` also keeps the mapping reads below type-safe -- the
 * previous code reached for `as unknown as Record<string, string>` casts, and
 * one of those casts is exactly what let an inverted mapping reach disk.
 */
type SnesButton = keyof ButtonMapping;

export interface WelcomeWizardProps {
  isOpen: boolean;
  onComplete: () => void;
  onClose: () => void;
  isRerun?: boolean;
}

/** The pad, in physical order — same grouping as the settings panel. */
const SNES_BUTTONS: Array<{ key: SnesButton; label: string }> = [
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

export function WelcomeWizard({
  isOpen,
  onComplete,
  onClose,
  isRerun = false,
}: WelcomeWizardProps) {
  const { settings, saveSettings } = useSettingsStore();

  const [currentStep, setCurrentStep] = useState<WizardStep>('welcome');
  const [formData, setFormData] = useState<WizardFormData>(DEFAULT_WIZARD_FORM_DATA);
  const [isScanning, setIsScanning] = useState(false);
  const [mappingKey, setMappingKey] = useState<SnesButton | null>(null);
  const [gamepads, setGamepads] = useState<(Gamepad | null)[]>([]);

  // Poll for gamepads only on the steps that show them.
  useEffect(() => {
    if (currentStep !== 'controller-type' && currentStep !== 'controller-profile') return;

    const read = () => setGamepads(Array.from(navigator.getGamepads()));
    read();
    const interval = setInterval(read, 500);
    return () => clearInterval(interval);
  }, [currentStep]);

  // Key capture for the mapping step.
  useEffect(() => {
    if (!mappingKey) return;

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.code === 'Escape') {
        setMappingKey(null);
        return;
      }

      setFormData((previous) => {
        // The wizard's own ButtonMapping is keyed button -> key code (the
        // opposite direction from the persisted `keyboard_mapping`); see
        // handleComplete, which inverts it before saving.
        const nextMapping: ButtonMapping = { ...previous.controllerProfile.buttonMapping };

        // Keep it a bijection: a key bound to one button must not stay bound to
        // another. Cleared bindings become '' rather than being deleted, since
        // ButtonMapping requires all twelve keys to be present.
        for (const button of Object.keys(nextMapping) as SnesButton[]) {
          if (button !== mappingKey && nextMapping[button] === e.code) {
            nextMapping[button] = '';
          }
        }
        nextMapping[mappingKey] = e.code;

        return {
          ...previous,
          controllerProfile: {
            ...previous.controllerProfile,
            buttonMapping: nextMapping,
          },
        };
      });

      setMappingKey(null);
    };

    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [mappingKey]);

  // Escape closes the wizard, except while a key capture is armed (there, Escape
  // cancels the capture -- handled by the listener above, which stops
  // propagation) and on the final step, where the only sensible action is to
  // finish.
  useEffect(() => {
    if (!isOpen || mappingKey || currentStep === 'complete') return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isOpen, mappingKey, currentStep, onClose]);

  const stepConfig = WIZARD_STEPS.find((s) => s.id === currentStep);
  const currentStepIndex = WIZARD_STEPS.findIndex((s) => s.id === currentStep);

  const nextStep = () => {
    const next = currentStepIndex + 1;
    if (next < WIZARD_STEPS.length) setCurrentStep(WIZARD_STEPS[next].id);
  };

  const prevStep = () => {
    const previous = currentStepIndex - 1;
    if (previous >= 0) setCurrentStep(WIZARD_STEPS[previous].id);
  };

  const handleComplete = async () => {
    try {
      const libraryChanges: typeof settings.library = {
        ...settings.library,
        use_metadata: formData.metadataSettings.enabled,
        artwork_source: formData.metadataSettings.preferredSource,
      };
      if (formData.romFolder) {
        libraryChanges.folders = [formData.romFolder];
        libraryChanges.scan_recursive = formData.scanSubfolders;
      }

      // The wizard collects `buttonMapping` as button -> key code, because that
      // is the direction a mapping UI reads in. `settings.controls
      // .keyboard_mapping` is the opposite: key code -> button, because that is
      // the direction a KeyboardEvent is looked up in.
      //
      // This used to be handed over with a straight `as unknown as
      // Record<string, string>` cast, which type-checked and silently persisted
      // an inverted map. EmulatorView then looked up 'ArrowUp' in its
      // button-name table, found nothing, and dropped every entry -- so anything
      // remapped during onboarding was quietly discarded and the user got the
      // built-in defaults instead.
      const keyboardMapping: Record<string, string> = {};
      for (const [button, code] of Object.entries(formData.controllerProfile.buttonMapping)) {
        if (code) keyboardMapping[code] = button;
      }

      const controlsChanges: typeof settings.controls =
        formData.controllerType === 'keyboard'
          ? {
              ...settings.controls,
              keyboard_enabled: true,
              gamepad_enabled: false,
              keyboard_mapping: keyboardMapping,
            }
          : {
              ...settings.controls,
              // Keyboard input stays on for a gamepad user: the hotkeys and the
              // menus are still driven by the keyboard, and turning it off left
              // people with a pad that had not been detected yet unable to play
              // at all.
              keyboard_enabled: true,
              gamepad_enabled: true,
            };

      // saveSettings replaces the whole settings object rather than merging, so
      // every step's values go in one call; sequential calls each spread from
      // the same stale `settings` would clobber all but the last.
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

      if (formData.romFolder.trim() !== '') {
        setIsScanning(true);
        try {
          await invoke('add_game_folder', {
            path: formData.romFolder,
            recursive: formData.scanSubfolders,
          });
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
        title: 'Select ROM folder',
      });
      if (typeof selected === 'string') {
        setFormData({ ...formData, romFolder: selected });
      }
    } catch (error) {
      console.error('Failed to select folder:', error);
    }
  };

  if (!isOpen) return null;

  const connectedPads = gamepads.filter(Boolean).length;
  const progress = ((currentStepIndex + 1) / WIZARD_STEPS.length) * 100;
  const mapping = formData.controllerProfile.buttonMapping;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div
        className="absolute inset-0"
        style={{ background: 'var(--scrim)', backdropFilter: 'blur(4px)' }}
        aria-hidden
      />

      <div
        className="panel animate-slide-in relative flex max-h-[calc(100vh-2rem)] w-full max-w-2xl flex-col overflow-hidden"
        role="dialog"
        aria-modal="true"
        aria-labelledby="wizard-title"
      >
        {/* Progress: the four-colour pinstripe filling left to right, so the
            app's signature element carries the one piece of state this dialog
            most needs to show. */}
        <div className="h-[3px] flex-none bg-line">
          <div
            className="sfc-pinstripe h-full transition-all duration-300"
            style={{ width: `${progress}%` }}
          />
        </div>

        <header className="flex-none border-b border-line px-6 pb-4 pt-5">
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <p className="microlabel">
                Step {currentStepIndex + 1} of {WIZARD_STEPS.length}
              </p>
              <h2 id="wizard-title" className="display-lg mt-1 text-ink">
                {stepConfig?.title}
              </h2>
              <p className="mt-1 text-sm text-dim">{stepConfig?.description}</p>
            </div>
            {/*
              An escape hatch. There was previously none at all: `onClose` was
              accepted as a prop and never wired to anything, so the wizard could
              only be left by walking all eight steps to the end -- including when
              it had been re-opened deliberately from Settings, where cancelling
              is the whole point.
            */}
            {currentStep !== 'complete' && (
              <button
                type="button"
                onClick={onClose}
                className="btn btn--ghost -mr-2 -mt-1 h-8 w-8 flex-none p-0"
                aria-label={isRerun ? 'Cancel setup' : 'Close setup for now'}
                title={isRerun ? 'Cancel setup' : 'Close for now'}
              >
                <IconClose size={16} />
              </button>
            )}
          </div>
        </header>

        <div className="min-h-[19rem] flex-1 overflow-y-auto px-6 py-5">
          {currentStep === 'welcome' && (
            <div className="py-8 text-center">
              <div className="mb-5 flex justify-center">
                <MarkSFC size={56} />
              </div>
              <h3 className="display-md text-ink">
                {isRerun ? 'Run setup again' : 'Welcome to OxideSFC'}
              </h3>
              <p className="mx-auto mt-2 max-w-md text-sm leading-relaxed text-dim">
                {isRerun
                  ? 'Step back through folder, controller and output setup. Nothing changes until you finish.'
                  : 'A Super Nintendo emulator with a from-scratch core. This takes about a minute: where your ROMs live, how you want to play, and how it should sound.'}
              </p>
            </div>
          )}

          {currentStep === 'language' && (
            <div className="max-w-sm space-y-4">
              <Select
                label="Interface language"
                value={formData.language}
                onChange={(e) => setFormData({ ...formData, language: e.target.value })}
                options={LANGUAGE_OPTIONS.map((lang) => ({
                  value: lang.code,
                  label: `${lang.nativeName} (${lang.name})`,
                }))}
                helperText="Changeable at any time under Settings › General."
              />
            </div>
          )}

          {currentStep === 'rom-folder' && (
            <div className="space-y-4">
              <p className="text-sm leading-relaxed text-dim">
                Point OxideSFC at a folder of ROMs. Files are never copied or
                moved — the library records where each one is and reads its
                cartridge header. You can add more folders later.
              </p>
              <div className="flex items-end gap-2">
                <Input
                  label="Folder"
                  value={formData.romFolder}
                  readOnly
                  placeholder="No folder selected"
                  leftIcon={<IconFolder size={15} />}
                />
                <Button variant="secondary" onClick={handleSelectFolder}>
                  Browse
                </Button>
              </div>
              <Toggle
                label="Include subfolders"
                description="Descend into nested folders while scanning."
                checked={formData.scanSubfolders}
                onChange={(e) =>
                  setFormData({ ...formData, scanSubfolders: e.target.checked })
                }
              />
            </div>
          )}

          {currentStep === 'controller-type' && (
            <div className="space-y-4">
              <p className="text-sm text-dim">How will you play?</p>
              <div className="grid grid-cols-2 gap-3">
                <button
                  type="button"
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
                  className={`rounded-lg border-2 p-6 text-center transition-colors ${
                    formData.controllerType === 'keyboard'
                      ? 'border-accent bg-accent-soft'
                      : 'border-line hover:border-line-strong'
                  }`}
                >
                  <span className="mb-2 block font-mono text-3xl text-accent-text">⌨</span>
                  <span className="block font-semibold text-ink">Keyboard</span>
                  <span className="hint mt-1 block">always available</span>
                </button>

                <button
                  type="button"
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
                  className={`rounded-lg border-2 p-6 text-center transition-colors ${
                    formData.controllerType === 'gamepad'
                      ? 'border-accent bg-accent-soft'
                      : 'border-line hover:border-line-strong'
                  }`}
                >
                  <span className="mb-2 flex justify-center text-accent-text">
                    <IconGamepad size={32} />
                  </span>
                  <span className="block font-semibold text-ink">Gamepad</span>
                  <span className="hint mt-1 block">
                    {connectedPads > 0 ? `${connectedPads} connected` : 'none detected yet'}
                  </span>
                </button>
              </div>
            </div>
          )}

          {currentStep === 'controller-profile' && (
            <div className="space-y-4">
              {formData.controllerType === 'keyboard' ? (
                <>
                  <p className="text-sm text-dim">
                    Click a key to rebind it, then press the key you want.
                  </p>
                  {mappingKey && (
                    <p className="rounded-md border border-accent-line bg-accent-soft px-3 py-2 text-center text-[0.8125rem] text-accent-text">
                      Press a key to bind to{' '}
                      <span className="font-semibold uppercase">{mappingKey}</span>, or
                      Esc to cancel.
                    </p>
                  )}
                  <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-3">
                    {SNES_BUTTONS.map((button) => {
                      const listening = mappingKey === button.key;
                      const code = mapping[button.key];
                      return (
                        <button
                          key={button.key}
                          type="button"
                          onClick={() => setMappingKey(button.key)}
                          disabled={mappingKey !== null && !listening}
                          className="flex items-center justify-between gap-2 rounded-md border border-line bg-raised px-2.5 py-2 transition-colors hover:border-accent-line disabled:opacity-40"
                        >
                          <span className="text-[0.8125rem] font-semibold text-ink">
                            {button.label}
                          </span>
                          <span
                            className={`keycap ${listening ? 'keycap--listening' : ''} ${
                              !code ? 'keycap--unbound' : ''
                            }`}
                          >
                            {listening ? 'press…' : code ? formatKeyCode(code) : 'none'}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                </>
              ) : (
                <>
                  <p className="text-sm leading-relaxed text-dim">
                    Gamepads use a standard layout and are picked up
                    automatically. Keyboard input stays enabled too, so menus and
                    hotkeys keep working.
                  </p>
                  <div className="rounded-md border border-line bg-raised px-3 py-3">
                    {connectedPads > 0 ? (
                      <p className="flex items-center gap-2 text-[0.8125rem] font-semibold text-success-text">
                        <IconCheck size={15} />
                        {connectedPads} gamepad{connectedPads > 1 ? 's' : ''} detected
                      </p>
                    ) : (
                      <p className="text-[0.8125rem] text-mute">
                        No gamepad yet. Connect one and press a button, or carry
                        on — it will be detected when you play.
                      </p>
                    )}
                  </div>
                  <p className="field-row-help">
                    Button layout and deadzone are under Settings › Controls.
                  </p>
                </>
              )}
            </div>
          )}

          {currentStep === 'audio-video' && (
            <div className="space-y-5">
              <div>
                <p className="eyebrow mb-2">PPU / OUTPUT</p>
                <div className="space-y-3">
                  <Toggle
                    label="Vertical sync"
                    description="Match the display's refresh to avoid tearing."
                    checked={formData.videoSettings.vsync}
                    onChange={(e) =>
                      setFormData({
                        ...formData,
                        videoSettings: { ...formData.videoSettings, vsync: e.target.checked },
                      })
                    }
                  />
                  <div className="max-w-xs">
                    <Select
                      label="Scale mode"
                      value={formData.videoSettings.scaleMode}
                      onChange={(e) =>
                        setFormData({
                          ...formData,
                          videoSettings: {
                            ...formData.videoSettings,
                            scaleMode: e.target.value,
                          },
                        })
                      }
                      // Only the modes the renderer implements. The wizard used
                      // to offer a "Canvas (CPU)" renderer as well, which does
                      // not exist -- there is one WebGL path.
                      options={[
                        { value: 'nearest', label: 'Nearest neighbour (sharp)' },
                        { value: 'bilinear', label: 'Bilinear (softer, CRT-like)' },
                      ]}
                      helperText="More upscalers, including xBRZ, are under Settings › Video."
                    />
                  </div>
                </div>
              </div>

              <div className="h-px bg-line" aria-hidden />

              <div>
                <p className="eyebrow mb-2">S-DSP / APU</p>
                <div className="space-y-3">
                  <Toggle
                    label="Sound"
                    checked={formData.audioSettings.enabled}
                    onChange={(e) =>
                      setFormData({
                        ...formData,
                        audioSettings: {
                          ...formData.audioSettings,
                          enabled: e.target.checked,
                        },
                      })
                    }
                  />
                  {formData.audioSettings.enabled && (
                    <div className="max-w-xs">
                      <Slider
                        label="Volume"
                        value={formData.audioSettings.volume}
                        min={0}
                        max={1}
                        step={0.05}
                        valueDisplay={(v) => `${Math.round(v * 100)}%`}
                        onChange={(e) =>
                          setFormData({
                            ...formData,
                            audioSettings: {
                              ...formData.audioSettings,
                              volume: parseFloat(e.target.value),
                            },
                          })
                        }
                      />
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}

          {currentStep === 'metadata' && (
            <div className="space-y-4">
              <p className="text-sm leading-relaxed text-dim">
                OxideSFC can look up cover art and release information online.
                Without it, games are drawn as cartridge labels tinted from their
                title — which many people prefer.
              </p>
              <Toggle
                label="Fetch metadata and artwork"
                description="Look up newly found games online."
                checked={formData.metadataSettings.enabled}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    metadataSettings: {
                      ...formData.metadataSettings,
                      enabled: e.target.checked,
                    },
                  })
                }
              />
              {formData.metadataSettings.enabled && (
                <div className="max-w-xs">
                  <Select
                    label="Preferred source"
                    value={formData.metadataSettings.preferredSource}
                    onChange={(e) =>
                      setFormData({
                        ...formData,
                        metadataSettings: {
                          ...formData.metadataSettings,
                          preferredSource: e.target.value,
                        },
                      })
                    }
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
            <div className="py-8 text-center">
              <div className="mb-4 flex justify-center text-success-text">
                <IconCheck size={44} />
              </div>
              <h3 className="display-md text-ink">Ready to play</h3>
              <p className="mx-auto mt-2 max-w-md text-sm leading-relaxed text-dim">
                {formData.romFolder
                  ? 'Finish to save your setup and scan the folder you picked.'
                  : 'Finish to save your setup. Add a ROM folder from the library whenever you like.'}
              </p>
              {isScanning && (
                <div className="mx-auto mt-5 max-w-xs">
                  <div className="h-1.5 overflow-hidden rounded-full bg-line">
                    <div className="sfc-pinstripe h-full animate-pulse" />
                  </div>
                  <p className="hint mt-2">scanning folder…</p>
                </div>
              )}
            </div>
          )}
        </div>

        <footer className="flex flex-none items-center justify-between border-t border-line px-6 py-4">
          <div>
            {currentStepIndex > 0 && currentStep !== 'complete' && (
              <Button variant="ghost" onClick={prevStep}>
                Back
              </Button>
            )}
          </div>
          <div className="flex gap-2">
            {currentStep !== 'welcome' &&
              currentStep !== 'complete' &&
              stepConfig?.canSkip && (
                <Button variant="ghost" onClick={nextStep}>
                  Skip
                </Button>
              )}
            {currentStep === 'complete' ? (
              <Button onClick={handleComplete} isLoading={isScanning}>
                {isScanning ? 'Scanning…' : 'Finish'}
              </Button>
            ) : currentStep !== 'controller-type' ? (
              <Button onClick={nextStep}>Continue</Button>
            ) : null}
          </div>
        </footer>
      </div>
    </div>
  );
}
