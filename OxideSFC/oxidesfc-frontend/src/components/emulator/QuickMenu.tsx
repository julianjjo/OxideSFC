import { useState, useEffect, useCallback, type RefObject } from 'react';
import { useEmulationStore } from '../../stores/emulationStore';
import { Button } from '../common/Button';
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
  theme?: 'dark' | 'light';
  /** Ref to the emulator's WebGL <canvas> (see EmulatorView.tsx), used to
   * capture the current frame for the Screenshot action. */
  canvasRef?: RefObject<HTMLCanvasElement | null>;
  /** Current game's title, used to build a sensible default screenshot
   * filename (e.g. "Super Mario World_2026-07-04T12-30-00.png"). */
  gameTitle?: string;
}

export function QuickMenu({
  isOpen,
  onClose,
  onOpenSettings,
  onExitToMenu,
  theme = 'dark',
  canvasRef,
  gameTitle,
}: QuickMenuProps) {
  const {
    isPaused,
    pause,
    resume,
    saveState,
    loadState,
  } = useEmulationStore();

  const [showSaveSlots, setShowSaveSlots] = useState(false);
  const [showLoadSlots, setShowLoadSlots] = useState(false);
  const [saveStatus, setSaveStatus] = useState<string | null>(null);
  const [screenshotPath, setScreenshotPath] = useState<string | null>(null);

  // Handle keyboard shortcuts
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      switch (e.key) {
        case 'Escape':
          onClose();
          break;
        case 'p':
        case 'P':
          handleTogglePause();
          break;
        case 'F5':
          handleQuickSave();
          break;
        case 'F9':
          handleQuickLoad();
          break;
        case 'F8':
          handleScreenshot();
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, isPaused]);

  const handleTogglePause = useCallback(async () => {
    if (isPaused) {
      await resume();
    } else {
      await pause();
    }
  }, [isPaused, pause, resume]);

  const handleQuickSave = useCallback(async () => {
    try {
      // Quick save to slot 0
      await saveState(0);
      setSaveStatus('Quick saved!');
      setTimeout(() => setSaveStatus(null), 2000);
    } catch (error) {
      console.error('Failed to quick save:', error);
      setSaveStatus('Save failed!');
      setTimeout(() => setSaveStatus(null), 2000);
    }
  }, [saveState]);

  const handleQuickLoad = useCallback(async () => {
    try {
      // Quick load from slot 0
      await loadState(0);
      setSaveStatus('Quick loaded!');
      setTimeout(() => setSaveStatus(null), 2000);
    } catch (error) {
      console.error('Failed to quick load:', error);
      setSaveStatus('Load failed!');
      setTimeout(() => setSaveStatus(null), 2000);
    }
  }, [loadState]);

  const handleSaveToSlot = async (slot: number) => {
    try {
      await saveState(slot);
      setSaveStatus(`Saved to slot ${slot + 1}!`);
      setShowSaveSlots(false);
      setTimeout(() => setSaveStatus(null), 2000);
    } catch (error) {
      console.error('Failed to save:', error);
      setSaveStatus('Save failed!');
      setTimeout(() => setSaveStatus(null), 2000);
    }
  };

  const handleLoadFromSlot = async (slot: number) => {
    try {
      await loadState(slot);
      setSaveStatus(`Loaded from slot ${slot + 1}!`);
      setShowLoadSlots(false);
      setTimeout(() => setSaveStatus(null), 2000);
    } catch (error) {
      console.error('Failed to load:', error);
      setSaveStatus('Load failed!');
      setTimeout(() => setSaveStatus(null), 2000);
    }
  };

  // Screenshot capture lives in captureScreenshot.ts, shared with the
  // control deck's button and the F8 gameplay hotkey (see EmulatorView).
  const handleScreenshot = async () => {
    const canvas = canvasRef?.current;
    if (!canvas) {
      console.error('Failed to take screenshot: no canvas available');
      setSaveStatus('Screenshot failed!');
      setTimeout(() => setSaveStatus(null), 2000);
      return;
    }

    try {
      const result = await captureScreenshot(canvas, gameTitle);
      if (result === 'saved') {
        setScreenshotPath('saved');
        setTimeout(() => setScreenshotPath(null), 3000);
      }
    } catch (error) {
      console.error('Failed to take screenshot:', error);
      setSaveStatus('Screenshot failed!');
      setTimeout(() => setSaveStatus(null), 2000);
    }
  };

  // The parent owns the full exit sequence (leave fullscreen, stop the
  // emulation, navigate back) -- see EmulatorView's handleStop.
  const handleExitToMenu = () => {
    onExitToMenu();
  };

  if (!isOpen) return null;

  const containerClass = theme === 'light'
    ? 'bg-white/95 backdrop-blur-sm'
    : 'bg-slate-900/95 backdrop-blur-sm';

  const textClass = theme === 'light'
    ? 'text-gray-800'
    : 'text-white';

  const mutedClass = theme === 'light'
    ? 'text-gray-500'
    : 'text-slate-400';

  const buttonClass = theme === 'light'
    ? 'hover:bg-gray-100'
    : 'hover:bg-slate-700';

  // Save/Load slots modal content
  if (showSaveSlots || showLoadSlots) {
    const slots = Array.from({ length: 10 }, (_, i) => i);
    
    return (
      <div className="fixed inset-0 flex items-center justify-center z-50">
        <div className={`absolute inset-0 bg-black/50`} onClick={() => {
          setShowSaveSlots(false);
          setShowLoadSlots(false);
        }} />
        
        <div className={`relative overflow-hidden rounded-xl p-6 max-w-sm w-full mx-4 ${containerClass}`}>
          <div className="sfc-pinstripe absolute top-0 inset-x-0 h-[3px]" aria-hidden />
          <h2 className={`text-xl font-semibold mb-4 ${textClass}`}>
            {showSaveSlots ? 'Save State' : 'Load State'}
          </h2>
          
          <div className="space-y-2">
            {slots.map((slot) => (
              <button
                key={slot}
                onClick={() => showSaveSlots ? handleSaveToSlot(slot) : handleLoadFromSlot(slot)}
                className={`w-full text-left px-4 py-3 rounded-lg flex items-center justify-between ${buttonClass}`}
              >
                <span className={textClass}>Slot {slot + 1}</span>
                <span className={mutedClass}>
                  {/* In a full implementation, would show save date */}
                  Empty
                </span>
              </button>
            ))}
          </div>
          
          <Button
            variant="ghost"
            className="mt-4 w-full"
            onClick={() => {
              setShowSaveSlots(false);
              setShowLoadSlots(false);
            }}
          >
            Cancel
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 flex items-center justify-center z-50">
      {/* Semi-transparent backdrop */}
      <div className="absolute inset-0 bg-black/50" onClick={onClose} />
      
      {/* Menu container */}
      <div className={`relative overflow-hidden rounded-xl p-6 max-w-md w-full mx-4 ${containerClass}`}>
        <div className="sfc-pinstripe absolute top-0 inset-x-0 h-[3px]" aria-hidden />
        {/* Header */}
        <div className="flex items-center justify-between mb-6">
          <div className="min-w-0">
            <h1 className={`text-2xl font-bold ${textClass}`}>Quick Menu</h1>
            {gameTitle && (
              <p className={`text-xs truncate ${mutedClass}`}>{gameTitle}</p>
            )}
          </div>
          <button
            onClick={onClose}
            className={`p-2 rounded-lg ${buttonClass} ${mutedClass}`}
            aria-label="Close menu"
            title="Close (Esc)"
          >
            <IconX />
          </button>
        </div>

        {/* Status message */}
        {saveStatus && (
          <div className={`mb-4 p-3 rounded-lg text-center ${
            theme === 'light' ? 'bg-green-100 text-green-700' : 'bg-green-900/30 text-green-400'
          }`}>
            {saveStatus}
          </div>
        )}

        {/* Screenshot notification */}
        {screenshotPath && (
          <div className={`mb-4 p-3 rounded-lg text-center ${
            theme === 'light' ? 'bg-blue-100 text-blue-700' : 'bg-blue-900/30 text-blue-400'
          }`}>
            Screenshot saved!
          </div>
        )}

        {/* Menu actions */}
        <div className="grid grid-cols-2 gap-3">
          {/* Pause/Resume */}
          <button
            onClick={handleTogglePause}
            className={`p-4 rounded-lg flex flex-col items-center gap-2 ${buttonClass}`}
          >
            <span style={{ color: 'var(--sfc-blue)' }}>
              {isPaused ? <IconPlay size={22} /> : <IconPause size={22} />}
            </span>
            <span className={textClass}>{isPaused ? 'Resume' : 'Pause'}</span>
            <span className={`text-xs ${mutedClass}`}>P</span>
          </button>

          {/* Quick Save */}
          <button
            onClick={handleQuickSave}
            className={`p-4 rounded-lg flex flex-col items-center gap-2 ${buttonClass}`}
          >
            <span style={{ color: 'var(--sfc-green)' }}>
              <IconSave size={22} />
            </span>
            <span className={textClass}>Quick Save</span>
            <span className={`text-xs ${mutedClass}`}>F5</span>
          </button>

          {/* Quick Load */}
          <button
            onClick={handleQuickLoad}
            className={`p-4 rounded-lg flex flex-col items-center gap-2 ${buttonClass}`}
          >
            <span style={{ color: 'var(--sfc-yellow)' }}>
              <IconLoad size={22} />
            </span>
            <span className={textClass}>Quick Load</span>
            <span className={`text-xs ${mutedClass}`}>F9</span>
          </button>

          {/* Save to Slot */}
          <button
            onClick={() => setShowSaveSlots(true)}
            className={`p-4 rounded-lg flex flex-col items-center gap-2 ${buttonClass} ${mutedClass}`}
          >
            <IconLayers size={22} />
            <span className={textClass}>Save Slot</span>
          </button>

          {/* Load from Slot */}
          <button
            onClick={() => setShowLoadSlots(true)}
            className={`p-4 rounded-lg flex flex-col items-center gap-2 ${buttonClass} ${mutedClass}`}
          >
            <IconFolderOpen size={22} />
            <span className={textClass}>Load Slot</span>
          </button>

          {/* Screenshot */}
          <button
            onClick={handleScreenshot}
            className={`p-4 rounded-lg flex flex-col items-center gap-2 ${buttonClass} ${mutedClass}`}
          >
            <IconCamera size={22} />
            <span className={textClass}>Screenshot</span>
            <span className={`text-xs ${mutedClass}`}>F8</span>
          </button>

          {/* Settings */}
          <button
            onClick={() => {
              onClose();
              onOpenSettings();
            }}
            className={`p-4 rounded-lg flex flex-col items-center gap-2 ${buttonClass} ${mutedClass}`}
          >
            <IconGear size={22} />
            <span className={textClass}>Settings</span>
          </button>

          {/* Exit to Menu */}
          <button
            onClick={handleExitToMenu}
            className={`p-4 rounded-lg flex flex-col items-center gap-2 ${buttonClass} col-span-2`}
          >
            <span style={{ color: 'var(--sfc-red)' }}>
              <IconHome size={22} />
            </span>
            <span className={textClass}>Exit to Menu</span>
          </button>
        </div>

        {/* Keyboard shortcuts hint */}
        <div className={`mt-6 text-center text-xs ${mutedClass}`}>
          Press ESC or click outside to close • P to toggle pause
        </div>
      </div>
    </div>
  );
}
