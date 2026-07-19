import { useState, useEffect } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import { DEFAULT_KEYBOARD_MAPPING } from '../../domain/keyboardDefaults';
import { Button } from '../common/Button';
import { Toggle } from '../common/Toggle';
import { Modal } from '../common/Modal';
import { Slider } from '../common/Slider';

// SNES button mapping labels
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

// Gamepad button mapping
interface GamepadState {
  connected: boolean;
  id: string;
  index: number;
  buttons: boolean[];
  axes: number[];
}

// Gamepad test button mapping
const GAMEPAD_BUTTONS = [
  { index: 0, label: 'A / Cross' },
  { index: 1, label: 'B / Circle' },
  { index: 2, label: 'X / Square' },
  { index: 3, label: 'Y / Triangle' },
  { index: 4, label: 'L1' },
  { index: 5, label: 'R1' },
  { index: 6, label: 'L2' },
  { index: 7, label: 'R2' },
  { index: 8, label: 'Select' },
  { index: 9, label: 'Start' },
  { index: 10, label: 'L3' },
  { index: 11, label: 'R3' },
  { index: 12, label: 'D-Pad Up' },
  { index: 13, label: 'D-Pad Down' },
  { index: 14, label: 'D-Pad Left' },
  { index: 15, label: 'D-Pad Right' },
];

export function ControllerSettings() {
  const { settings, saveSettings } = useSettingsStore();
  const theme = settings.general.theme;
  const controls = settings.controls;

  const [mappingKey, setMappingKey] = useState<string | null>(null);
  const [isListening, setIsListening] = useState(false);
  const [gamepads, setGamepads] = useState<GamepadState[]>([]);
  const [testMode, setTestMode] = useState(false);
  const [deadzone, setDeadzone] = useState(0.1);
  const [showResetConfirm, setShowResetConfirm] = useState(false);

  // Get current key mappings
  const keyboardMapping = controls.keyboard_mapping || {};

  // Poll for gamepads
  useEffect(() => {
    const updateGamepads = () => {
      const gamepadList: GamepadState[] = [];
      const n = navigator.getGamepads();
      
      for (let i = 0; i < n.length; i++) {
        const gp = n[i];
        if (gp) {
          gamepadList.push({
            connected: true,
            id: gp.id,
            index: i,
            buttons: gp.buttons.map(b => b.pressed),
            axes: Array.from(gp.axes),
          });
        }
      }
      
      setGamepads(gamepadList);
    };

    // Initial check
    updateGamepads();

    // Poll for changes
    const interval = setInterval(updateGamepads, 500);
    return () => clearInterval(interval);
  }, []);

  // Handle key press for remapping
  useEffect(() => {
    if (!isListening || !mappingKey) return;

    const handleKeyDown = async (e: KeyboardEvent) => {
      e.preventDefault();

      const keyCode = e.code;

      // Escape cancels remapping instead of being bound as the new key --
      // the UI's "(ESC to cancel)" hint has to actually be true.
      if (keyCode === 'Escape') {
        setIsListening(false);
        setMappingKey(null);
        return;
      }

      // Check if already in use
      const existingBinding = Object.entries(keyboardMapping).find(
        ([, value]) => value === mappingKey
      );

      // Update mapping - remove old key for this action if exists
      const newMapping = { ...keyboardMapping };

      // Remove existing binding for this action
      if (existingBinding) {
        delete newMapping[existingBinding[0]];
      }

      // Remove key if it's already bound to another action
      if (newMapping[keyCode]) {
        delete newMapping[keyCode];
      }

      // Add new mapping
      newMapping[keyCode] = mappingKey;

      // Save to store
      await saveSettings({
        ...settings,
        controls: {
          ...controls,
          keyboard_mapping: newMapping,
        },
      });

      setIsListening(false);
      setMappingKey(null);
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isListening, mappingKey, keyboardMapping, settings, controls, saveSettings]);

  // Handle remapping click
  const handleRemapClick = (action: string) => {
    setMappingKey(action);
    setIsListening(true);
  };

  // Reset to defaults
  const handleResetDefaults = async () => {
    await saveSettings({
      ...settings,
      controls: {
        ...controls,
        keyboard_mapping: DEFAULT_KEYBOARD_MAPPING,
      },
    });
    setShowResetConfirm(false);
  };

  // Get key label for display
  const getKeyLabel = (action: string): string => {
    const entry = Object.entries(keyboardMapping).find(([, value]) => value === action);
    if (!entry) return 'Not bound';
    
    const keyCode = entry[0];
    // Format key code for display
    return keyCode
      .replace('Key', '')
      .replace('Digit', '')
      .replace('Arrow', '')
      .replace('Left', 'L-')
      .replace('Right', 'R-');
  };

  // Toggle keyboard input
  const handleKeyboardToggle = async (enabled: boolean) => {
    await saveSettings({
      ...settings,
      controls: {
        ...controls,
        keyboard_enabled: enabled,
      },
    });
  };

  // Toggle gamepad input
  const handleGamepadToggle = async (enabled: boolean) => {
    await saveSettings({
      ...settings,
      controls: {
        ...controls,
        gamepad_enabled: enabled,
      },
    });
  };

  // Handle gamepad profile change
  const handleProfileChange = async (profile: string) => {
    await saveSettings({
      ...settings,
      controls: {
        ...controls,
        gamepad_profile: profile,
      },
    });
  };

  // Handle deadzone change
  const handleDeadzoneChange = async (value: number) => {
    setDeadzone(value);
  };

  return (
    <div className="space-y-6">
      {/* Keyboard Configuration */}
      <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">Keyboard Configuration</h2>
          <Toggle
            checked={controls.keyboard_enabled}
            onChange={(e) => handleKeyboardToggle(e.target.checked)}
            label="Enabled"
          />
        </div>

        {isListening && (
          <div className={`mb-4 p-3 rounded-lg text-center ${
            theme === 'light' ? 'bg-blue-50 text-blue-700' : 'bg-blue-900/30 text-blue-300'
          }`}>
            Press any key to map to "{mappingKey}" (ESC to cancel)
          </div>
        )}

        <div className="overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr className={theme === 'light' ? 'border-b border-gray-200' : 'border-b border-slate-700'}>
                <th className={`text-left py-2 px-3 font-medium ${
                  theme === 'light' ? 'text-gray-600' : 'text-slate-400'
                }`}>Action</th>
                <th className={`text-left py-2 px-3 font-medium ${
                  theme === 'light' ? 'text-gray-600' : 'text-slate-400'
                }`}>Key</th>
                <th className={`text-right py-2 px-3 font-medium ${
                  theme === 'light' ? 'text-gray-600' : 'text-slate-400'
                }`}>Remap</th>
              </tr>
            </thead>
            <tbody>
              {SNES_BUTTONS.map((button) => (
                <tr key={button.key} className={
                  theme === 'light' ? 'border-b border-gray-100' : 'border-b border-slate-700/50'
                }>
                  <td className={`py-2 px-3 ${
                    theme === 'light' ? 'text-gray-700' : 'text-slate-300'
                  }`}>{button.label}</td>
                  <td className={`py-2 px-3 font-mono text-sm ${
                    theme === 'light' ? 'text-gray-900' : 'text-slate-100'
                  }`}>
                    {isListening && mappingKey === button.key ? (
                      <span className="text-blue-500 animate-pulse">Press key...</span>
                    ) : (
                      getKeyLabel(button.key)
                    )}
                  </td>
                  <td className="py-2 px-3 text-right">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => handleRemapClick(button.key)}
                      disabled={isListening && mappingKey !== button.key}
                    >
                      {isListening && mappingKey === button.key ? '...' : 'Remap'}
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="mt-4 flex justify-end">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => setShowResetConfirm(true)}
          >
            Reset to Defaults
          </Button>
        </div>
      </section>

      {/* Gamepad Configuration */}
      <section className={`rounded-lg p-6 ${theme === 'light' ? 'bg-white' : 'bg-slate-800'}`}>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">Gamepad Configuration</h2>
          <Toggle
            checked={controls.gamepad_enabled}
            onChange={(e) => handleGamepadToggle(e.target.checked)}
            label="Enabled"
          />
        </div>

        {/* Connected Gamepads */}
        <div className="mb-4">
          <h3 className={`text-sm font-medium mb-2 ${
            theme === 'light' ? 'text-gray-700' : 'text-slate-300'
          }`}>Connected Gamepads</h3>
          {gamepads.length === 0 ? (
            <p className={`text-sm ${
              theme === 'light' ? 'text-gray-500' : 'text-slate-400'
            }`}>No gamepads connected</p>
          ) : (
            <div className="space-y-2">
              {gamepads.map((gp) => (
                <div key={gp.index} className={`p-2 rounded-lg ${
                  theme === 'light' ? 'bg-gray-100' : 'bg-slate-700'
                }`}>
                  <div className="flex items-center justify-between">
                    <span className={`text-sm ${
                      theme === 'light' ? 'text-gray-700' : 'text-slate-200'
                    }`}>
                      Gamepad {gp.index + 1}
                    </span>
                    <span className="text-xs text-green-500">Connected</span>
                  </div>
                  <p className={`text-xs truncate ${
                    theme === 'light' ? 'text-gray-500' : 'text-slate-400'
                  }`}>{gp.id}</p>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Test Gamepad */}
        <div className="mb-4">
          <Button
            variant={testMode ? 'primary' : 'secondary'}
            size="sm"
            onClick={() => setTestMode(!testMode)}
          >
            {testMode ? 'Stop Test' : 'Test Gamepad'}
          </Button>
          
          {testMode && gamepads.length > 0 && (
            <div className="mt-3 grid grid-cols-4 gap-2">
              {GAMEPAD_BUTTONS.map((btn) => {
                const pressed = gamepads[0].buttons[btn.index];
                return (
                  <div
                    key={btn.index}
                    className={`p-2 rounded text-center text-xs transition-colors ${
                      pressed
                        ? theme === 'light'
                          ? 'bg-blue-600 text-white'
                          : 'bg-blue-500 text-white'
                        : theme === 'light'
                          ? 'bg-gray-100 text-gray-600'
                          : 'bg-slate-700 text-slate-400'
                    }`}
                  >
                    {btn.label}
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Profile Selection */}
        <div className="mb-4">
          <label className={`block text-sm font-medium mb-2 ${
            theme === 'light' ? 'text-gray-700' : 'text-slate-300'
          }`}>Profile</label>
          <select
            value={controls.gamepad_profile}
            onChange={(e) => handleProfileChange(e.target.value)}
            className={`w-full px-3 py-2 rounded-lg ${
              theme === 'light'
                ? 'bg-gray-100 border border-gray-300'
                : 'bg-slate-700 border border-slate-600'
            }`}
          >
            <option value="default">Default</option>
            <option value="xbox">Xbox</option>
            <option value="playstation">PlayStation</option>
            <option value="switch">Nintendo Switch</option>
          </select>
        </div>

        {/* Deadzone Settings */}
        <div>
          <Slider
            label="Deadzone"
            value={deadzone}
            min={0}
            max={0.5}
            step={0.05}
            showValue
            valueDisplay={(v) => `${Math.round(v * 100)}%`}
            onChange={(e) => handleDeadzoneChange(parseFloat(e.target.value))}
          />
        </div>
      </section>

      {/* Reset Confirmation Modal */}
      <Modal
        isOpen={showResetConfirm}
        onClose={() => setShowResetConfirm(false)}
        title="Reset Keyboard Mappings"
        footer={
          <>
            <Button variant="ghost" onClick={() => setShowResetConfirm(false)}>
              Cancel
            </Button>
            <Button variant="danger" onClick={handleResetDefaults}>
              Reset
            </Button>
          </>
        }
      >
        <p>Are you sure you want to reset all keyboard mappings to their defaults?</p>
      </Modal>
    </div>
  );
}