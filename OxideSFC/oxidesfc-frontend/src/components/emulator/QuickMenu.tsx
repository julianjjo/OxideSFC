import { useState, useEffect, useCallback, type RefObject } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useEmulationStore } from '../../stores/emulationStore';
import { Button } from '../common/Button';
import { Modal } from '../common/Modal';
import { captureScreenshot } from './captureScreenshot';
import {
  IconPlay,
  IconPause,
  IconSave,
  IconLoad,
  IconCamera,
  IconGear,
  IconHome,
  IconLayers,
  IconFolderOpen,
  IconX,
} from './icons';

interface QuickMenuProps {
  isOpen: boolean;
  onClose: () => void;
  onOpenSettings: () => void;
  onExitToMenu: () => void;
  /** Ref to the emulator's WebGL <canvas>, used to capture the current frame. */
  canvasRef?: RefObject<HTMLCanvasElement | null>;
  gameTitle?: string;
}

interface SaveSlotInfo {
  slot: number;
  occupied: boolean;
  size_bytes: number | null;
  saved_at_ms: number | null;
}

function formatSlotStamp(ms: number | null): string {
  if (!ms) return '';
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

export function QuickMenu({
  isOpen,
  onClose,
  onOpenSettings,
  onExitToMenu,
  canvasRef,
  gameTitle,
}: QuickMenuProps) {
  const { isPaused, pause, resume, saveState, loadState } = useEmulationStore();

  const [slotPicker, setSlotPicker] = useState<'save' | 'load' | null>(null);
  const [slots, setSlots] = useState<SaveSlotInfo[]>([]);
  const [status, setStatus] = useState<{ tone: 'ok' | 'err'; text: string } | null>(null);

  const flash = useCallback((text: string, tone: 'ok' | 'err' = 'ok') => {
    setStatus({ tone, text });
    window.setTimeout(() => setStatus(null), 2000);
  }, []);

  const refreshSlots = useCallback(async () => {
    try {
      setSlots(await invoke<SaveSlotInfo[]>('list_save_states'));
    } catch (error) {
      console.error('Failed to list save states:', error);
      setSlots([]);
    }
  }, []);

  // Read slot occupancy whenever a picker opens, so a state written moments ago
  // in this same session shows up.
  useEffect(() => {
    if (slotPicker) void refreshSlots();
  }, [slotPicker, refreshSlots]);

  const handleTogglePause = useCallback(async () => {
    if (isPaused) await resume();
    else await pause();
  }, [isPaused, pause, resume]);

  const handleQuickSave = useCallback(async () => {
    try {
      await saveState(0);
      flash('Saved to slot 1');
    } catch (error) {
      console.error('Failed to quick save:', error);
      flash('Save failed', 'err');
    }
  }, [saveState, flash]);

  const handleQuickLoad = useCallback(async () => {
    try {
      await loadState(0);
      flash('Loaded slot 1');
    } catch (error) {
      console.error('Failed to quick load:', error);
      flash('Nothing saved in slot 1', 'err');
    }
  }, [loadState, flash]);

  const handleScreenshot = useCallback(async () => {
    const canvas = canvasRef?.current;
    if (!canvas) {
      flash('Screenshot failed', 'err');
      return;
    }
    try {
      const result = await captureScreenshot(canvas, gameTitle);
      if (result === 'saved') flash('Screenshot saved');
    } catch (error) {
      console.error('Failed to take screenshot:', error);
      flash('Screenshot failed', 'err');
    }
  }, [canvasRef, gameTitle, flash]);

  // Hotkeys while the menu is open. `handleTogglePause` and friends are
  // dependencies rather than being captured once, so the handler never acts on a
  // stale `isPaused` (the previous version listed only [isOpen, isPaused] while
  // calling functions defined below it, which read a stale closure).
  useEffect(() => {
    if (!isOpen) return;

    const onKeyDown = (e: KeyboardEvent) => {
      switch (e.key) {
        case 'Escape':
          if (slotPicker) setSlotPicker(null);
          else onClose();
          break;
        case 'p':
        case 'P':
          void handleTogglePause();
          break;
        case 'F5':
          e.preventDefault();
          void handleQuickSave();
          break;
        case 'F9':
          e.preventDefault();
          void handleQuickLoad();
          break;
        case 'F8':
          e.preventDefault();
          void handleScreenshot();
          break;
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [
    isOpen,
    slotPicker,
    onClose,
    handleTogglePause,
    handleQuickSave,
    handleQuickLoad,
    handleScreenshot,
  ]);

  const handleSlotAction = async (slot: number) => {
    const saving = slotPicker === 'save';
    try {
      if (saving) {
        await saveState(slot);
        flash(`Saved to slot ${slot + 1}`);
      } else {
        await loadState(slot);
        flash(`Loaded slot ${slot + 1}`);
      }
      setSlotPicker(null);
    } catch (error) {
      console.error(saving ? 'Failed to save:' : 'Failed to load:', error);
      flash(saving ? 'Save failed' : 'Load failed', 'err');
    }
  };

  if (!isOpen) return null;

  const tiles: Array<{
    label: string;
    hint?: string;
    icon: React.ReactNode;
    tint?: string;
    onClick: () => void;
    span?: boolean;
  }> = [
    {
      label: isPaused ? 'Resume' : 'Pause',
      hint: 'P',
      icon: isPaused ? <IconPlay size={20} /> : <IconPause size={20} />,
      tint: 'var(--sfc-blue)',
      onClick: () => void handleTogglePause(),
    },
    {
      label: 'Quick save',
      hint: 'F5',
      icon: <IconSave size={20} />,
      tint: 'var(--sfc-green)',
      onClick: () => void handleQuickSave(),
    },
    {
      label: 'Quick load',
      hint: 'F9',
      icon: <IconLoad size={20} />,
      tint: 'var(--sfc-yellow)',
      onClick: () => void handleQuickLoad(),
    },
    {
      label: 'Save to slot',
      icon: <IconLayers size={20} />,
      onClick: () => setSlotPicker('save'),
    },
    {
      label: 'Load from slot',
      icon: <IconFolderOpen size={20} />,
      onClick: () => setSlotPicker('load'),
    },
    {
      label: 'Screenshot',
      hint: 'F8',
      icon: <IconCamera size={20} />,
      onClick: () => void handleScreenshot(),
    },
    {
      label: 'Settings',
      icon: <IconGear size={20} />,
      onClick: () => {
        onClose();
        onOpenSettings();
      },
    },
    {
      label: 'Exit to library',
      icon: <IconHome size={20} />,
      tint: 'var(--sfc-red)',
      onClick: onExitToMenu,
      span: true,
    },
  ];

  return (
    <>
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <div
          className="absolute inset-0 animate-fade-in"
          style={{ background: 'var(--scrim)', backdropFilter: 'blur(3px)' }}
          onClick={onClose}
          aria-hidden
        />

        <div
          className="panel pinstripe-top animate-slide-in relative w-full max-w-md overflow-hidden p-5"
          role="dialog"
          aria-modal="true"
          aria-label="Quick menu"
        >
          <div className="mb-4 flex items-start justify-between gap-4">
            <div className="min-w-0">
              <p className="eyebrow">Paused menu</p>
              <h2 className="display-md mt-1 truncate text-ink">
                {gameTitle || 'Quick menu'}
              </h2>
            </div>
            <button
              onClick={onClose}
              className="btn btn--ghost -mr-1 h-8 w-8 flex-none p-0"
              aria-label="Close menu"
              title="Close (Esc)"
            >
              <IconX size={16} />
            </button>
          </div>

          {status && (
            <p
              className={`mb-3 rounded-md border px-3 py-2 text-center text-[0.8125rem] ${
                status.tone === 'err'
                  ? 'border-danger-line bg-danger-soft text-danger-text'
                  : 'border-success-line bg-success-soft text-success-text'
              }`}
              role="status"
            >
              {status.text}
            </p>
          )}

          <div className="grid grid-cols-2 gap-2">
            {tiles.map((tile) => (
              <button
                key={tile.label}
                type="button"
                onClick={tile.onClick}
                className={`qm-tile ${tile.span ? 'col-span-2' : ''}`}
              >
                <span style={tile.tint ? { color: tile.tint } : undefined}>{tile.icon}</span>
                <span>{tile.label}</span>
                {tile.hint && <span className="register">{tile.hint}</span>}
              </button>
            ))}
          </div>

          <p className="hint mt-4 text-center">Esc or click outside to close</p>
        </div>
      </div>

      <Modal
        isOpen={slotPicker !== null}
        onClose={() => setSlotPicker(null)}
        title={slotPicker === 'save' ? 'Save state' : 'Load state'}
        subtitle={gameTitle}
        size="sm"
        footer={
          <Button variant="ghost" onClick={() => setSlotPicker(null)}>
            Cancel
          </Button>
        }
      >
        <ul className="space-y-1">
          {slots.map((slot) => {
            // Loading a free slot cannot do anything, so it is not offered.
            // Saving over an occupied one is allowed and says so.
            const disabled = slotPicker === 'load' && !slot.occupied;
            return (
              <li key={slot.slot}>
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() => void handleSlotAction(slot.slot)}
                  className="flex w-full items-center justify-between gap-3 rounded-md border border-line bg-raised px-3 py-2 text-left transition-colors hover:border-accent-line enabled:hover:bg-accent-soft disabled:opacity-45"
                >
                  <span className="text-[0.8125rem] font-semibold text-ink">
                    Slot {slot.slot + 1}
                  </span>
                  <span className="register">
                    {slot.occupied
                      ? `${formatSlotStamp(slot.saved_at_ms)}${
                          slotPicker === 'save' ? ' · overwrite' : ''
                        }`
                      : 'empty'}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>

        <p className="field-row-help mt-3">
          Slots are shared across games: saving here replaces whatever slot held,
          whichever cartridge wrote it. A state only loads back into the game it
          came from.
        </p>
      </Modal>
    </>
  );
}
