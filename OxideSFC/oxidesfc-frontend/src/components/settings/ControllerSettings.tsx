import { useState, useEffect, useMemo } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import { DEFAULT_KEYBOARD_MAPPING } from '../../domain/keyboardDefaults';
import { bindingsByButton, formatKeyCode } from '../../domain/keyLabel';
import { Button } from '../common/Button';
import { Toggle } from '../common/Toggle';
import { Select } from '../common/Select';
import { Slider } from '../common/Slider';
import { ConfirmModal } from '../common/Modal';
import { SettingsSection, SettingRow, SettingBlock, SettingNote } from './SettingsSection';

/**
 * The pad, grouped the way it is physically laid out rather than alphabetically:
 * directions first, then the diamond of face buttons, then shoulders, then the
 * centre pair. Finding "Y" is a matter of looking where Y sits on the hardware.
 */
const BUTTON_GROUPS: Array<{ group: string; buttons: Array<{ key: string; label: string }> }> = [
  {
    group: 'D-pad',
    buttons: [
      { key: 'up', label: 'Up' },
      { key: 'down', label: 'Down' },
      { key: 'left', label: 'Left' },
      { key: 'right', label: 'Right' },
    ],
  },
  {
    group: 'Face buttons',
    buttons: [
      { key: 'a', label: 'A' },
      { key: 'b', label: 'B' },
      { key: 'x', label: 'X' },
      { key: 'y', label: 'Y' },
    ],
  },
  {
    group: 'Shoulders',
    buttons: [
      { key: 'l', label: 'L' },
      { key: 'r', label: 'R' },
    ],
  },
  {
    group: 'Centre',
    buttons: [
      { key: 'start', label: 'Start' },
      { key: 'select', label: 'Select' },
    ],
  },
];

const PROFILE_OPTIONS = [
  { value: 'default', label: 'Default' },
  { value: 'xbox', label: 'Xbox' },
  { value: 'playstation', label: 'PlayStation' },
  { value: 'switch', label: 'Nintendo Switch' },
];

/** Standard-gamepad indices, used only by the input test below. */
const GAMEPAD_TEST_BUTTONS = [
  { index: 12, label: 'Up' },
  { index: 13, label: 'Down' },
  { index: 14, label: 'Left' },
  { index: 15, label: 'Right' },
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
];

interface PadState {
  id: string;
  index: number;
  buttons: boolean[];
  axes: number[];
}

export function ControllerSettings() {
  const { settings, updateSection } = useSettingsStore();
  const controls = settings.controls;
  const keyboardMapping = controls.keyboard_mapping || {};

  const [listeningFor, setListeningFor] = useState<string | null>(null);
  const [pads, setPads] = useState<PadState[]>([]);
  const [testMode, setTestMode] = useState(false);
  const [showResetConfirm, setShowResetConfirm] = useState(false);

  const patchControls = (patch: Partial<typeof controls>) => updateSection('controls', patch);

  /** SNES button -> bound key code. */
  const boundKeys = useMemo(() => bindingsByButton(keyboardMapping), [keyboardMapping]);

  // Poll connected pads. While the test grid is open this has to run at frame
  // rate to feel responsive; otherwise once a second is plenty just to notice a
  // pad being plugged in.
  useEffect(() => {
    let raf = 0;
    let timer = 0;

    const read = () => {
      const next: PadState[] = [];
      for (const pad of navigator.getGamepads()) {
        if (!pad) continue;
        next.push({
          id: pad.id,
          index: pad.index,
          buttons: pad.buttons.map((b) => b.pressed),
          axes: Array.from(pad.axes),
        });
      }
      setPads(next);
    };

    read();
    if (testMode) {
      const loop = () => {
        read();
        raf = requestAnimationFrame(loop);
      };
      raf = requestAnimationFrame(loop);
    } else {
      timer = window.setInterval(read, 1000);
    }

    return () => {
      if (raf) cancelAnimationFrame(raf);
      if (timer) window.clearInterval(timer);
    };
  }, [testMode]);

  // Key capture for remapping.
  useEffect(() => {
    if (!listeningFor) return;

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      // Escape cancels rather than being bound -- the "Esc to cancel" hint has
      // to actually be true.
      if (e.code === 'Escape') {
        setListeningFor(null);
        return;
      }

      const next: Record<string, string> = { ...keyboardMapping };
      // Drop this button's previous key, and steal the pressed key from
      // whatever else held it, so the map stays a bijection and no button ends
      // up sharing a key with another.
      for (const [code, button] of Object.entries(next)) {
        if (button === listeningFor) delete next[code];
      }
      delete next[e.code];
      next[e.code] = listeningFor;

      void patchControls({ keyboard_mapping: next });
      setListeningFor(null);
    };

    // Capture phase: the emulator's own global key handlers are on window too,
    // and a bare listener here would let them see the keystroke first.
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [listeningFor, keyboardMapping, settings, controls]);

  const unboundCount = BUTTON_GROUPS.flatMap((g) => g.buttons).filter(
    (b) => !boundKeys[b.key]
  ).length;

  // Held locally while dragging so the thumb tracks the pointer instead of a
  // disk round-trip: binding `value` straight to the persisted setting queued
  // ~50 serialised settings.json rewrites for one sweep across the range.
  // Persisted on release (`onPointerUp`/`onKeyUp`), which is the same shape the
  // audio panel's sliders use.
  const [deadzoneDraft, setDeadzoneDraft] = useState<number | null>(null);
  const persistedDeadzone = controls.gamepad_deadzone ?? 0.1;
  const deadzone = deadzoneDraft ?? persistedDeadzone;

  const commitDeadzone = () => {
    if (deadzoneDraft === null || deadzoneDraft === persistedDeadzone) {
      setDeadzoneDraft(null);
      return;
    }
    void patchControls({ gamepad_deadzone: deadzoneDraft });
    setDeadzoneDraft(null);
  };

  return (
    <div className="space-y-4">
      <SettingsSection
        eyebrow="JOYPAD 1 · KEYBOARD"
        title="Keyboard mapping"
        action={
          <Toggle
            checked={controls.keyboard_enabled}
            onChange={(e) => patchControls({ keyboard_enabled: e.target.checked })}
            aria-label="Enable keyboard input"
          />
        }
        description="Click a key to rebind it, then press the key you want. Bindings apply immediately, including to a game already running."
      >
        {listeningFor && (
          <SettingNote tone="accent">
            Press a key to bind to{' '}
            <span className="font-semibold uppercase">{listeningFor}</span>, or Esc
            to cancel.
          </SettingNote>
        )}

        {unboundCount > 0 && !listeningFor && (
          <SettingNote tone="danger">
            {unboundCount} {unboundCount === 1 ? 'button has' : 'buttons have'} no
            key bound and cannot be pressed. Games that need{' '}
            {unboundCount === 1 ? 'it' : 'them'} will be unplayable.
          </SettingNote>
        )}

        <div className="mt-3 space-y-3">
          {BUTTON_GROUPS.map((group) => (
            <div key={group.group}>
              <p className="eyebrow mb-1.5">{group.group}</p>
              <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-4">
                {group.buttons.map((button) => {
                  const code = boundKeys[button.key];
                  const listening = listeningFor === button.key;
                  return (
                    <button
                      key={button.key}
                      type="button"
                      onClick={() => setListeningFor(button.key)}
                      disabled={listeningFor !== null && !listening}
                      className={`flex items-center justify-between gap-2 rounded-md border border-line bg-raised px-2.5 py-2 text-left transition-colors hover:border-accent-line disabled:opacity-40 ${
                        listening ? 'border-accent' : ''
                      }`}
                      aria-label={`Rebind ${button.label}`}
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
            </div>
          ))}
        </div>

        <div className="mt-4 flex justify-end">
          <Button variant="secondary" size="sm" onClick={() => setShowResetConfirm(true)}>
            Restore defaults
          </Button>
        </div>
      </SettingsSection>

      <SettingsSection
        eyebrow="JOYPAD 1-2 · GAMEPAD"
        title="Gamepad"
        action={
          <Toggle
            checked={controls.gamepad_enabled}
            onChange={(e) => patchControls({ gamepad_enabled: e.target.checked })}
            aria-label="Enable gamepad input"
          />
        }
      >
        <SettingRow
          label="Connected"
          help={
            pads.length === 0
              ? 'Plug in a pad and press a button — some pads only announce themselves once used.'
              : undefined
          }
        >
          {pads.length === 0 ? (
            <span className="register">none detected</span>
          ) : (
            <span className="chip chip--accent">
              {pads.length} {pads.length === 1 ? 'pad' : 'pads'}
            </span>
          )}
        </SettingRow>

        {pads.map((pad) => (
          <SettingRow key={pad.index} label={`Pad ${pad.index + 1}`}>
            <span className="register max-w-xs truncate text-ink" title={pad.id}>
              {pad.id}
            </span>
          </SettingRow>
        ))}

        <SettingRow
          label="Button layout"
          help="Which physical layout your pad's face buttons follow."
        >
          <Select
            options={PROFILE_OPTIONS}
            value={controls.gamepad_profile}
            onChange={(e) => patchControls({ gamepad_profile: e.target.value })}
            inputSize="sm"
            className="w-48"
            aria-label="Button layout"
          />
        </SettingRow>

        <SettingBlock>
          <Slider
            label="Stick deadzone"
            min={0}
            max={0.5}
            step={0.01}
            value={deadzone}
            showMinMax
            valueDisplay={(v) => `${Math.round(v * 100)}%`}
            onChange={(e) => setDeadzoneDraft(parseFloat(e.target.value))}
            onPointerUp={commitDeadzone}
            onKeyUp={commitDeadzone}
            onBlur={commitDeadzone}
            helperText="How far a stick must move before it registers. Raise it if a worn stick makes your character drift while untouched. Applies to the next game you start."
          />
        </SettingBlock>

        <SettingRow
          label="Input test"
          help="Press buttons on the pad to confirm the app sees them."
        >
          <Button
            variant={testMode ? 'primary' : 'secondary'}
            size="sm"
            onClick={() => setTestMode(!testMode)}
            disabled={pads.length === 0}
          >
            {testMode ? 'Stop test' : 'Start test'}
          </Button>
        </SettingRow>

        {testMode && pads.length > 0 && (
          <div className="mt-3">
            <div className="grid grid-cols-4 gap-1.5">
              {GAMEPAD_TEST_BUTTONS.map((button) => {
                const pressed = pads[0].buttons[button.index];
                return (
                  <div
                    key={button.index}
                    className={`rounded-md border px-2 py-1.5 text-center text-[0.6875rem] font-semibold transition-colors ${
                      pressed
                        ? 'border-accent bg-accent text-accent-on'
                        : 'border-line bg-raised text-mute'
                    }`}
                  >
                    {button.label}
                  </div>
                );
              })}
            </div>
            <div className="mt-2 grid grid-cols-2 gap-3">
              {[0, 2].map((axisBase) => (
                <div key={axisBase} className="flex items-center gap-2">
                  <span className="microlabel">
                    {axisBase === 0 ? 'Left stick' : 'Right stick'}
                  </span>
                  <span className="register text-ink">
                    {(pads[0].axes[axisBase] ?? 0).toFixed(2)},{' '}
                    {(pads[0].axes[axisBase + 1] ?? 0).toFixed(2)}
                  </span>
                </div>
              ))}
            </div>
          </div>
        )}
      </SettingsSection>

      <ConfirmModal
        isOpen={showResetConfirm}
        onClose={() => setShowResetConfirm(false)}
        onConfirm={() => {
          void patchControls({ keyboard_mapping: DEFAULT_KEYBOARD_MAPPING });
          setShowResetConfirm(false);
        }}
        title="Restore default keys"
        message="Every keyboard binding goes back to the shipped layout. Your gamepad settings are not affected."
        confirmText="Restore defaults"
        variant="danger"
      />
    </div>
  );
}
