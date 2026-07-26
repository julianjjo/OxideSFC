import type { PointerEvent, FocusEvent, ReactNode } from 'react';
import {
  IconPlay,
  IconPause,
  IconSave,
  IconLoad,
  IconCamera,
  IconGrid,
  IconExpand,
  IconContract,
  IconPower,
  IconMinus,
  IconPlus,
} from './icons';

interface ControlDeckProps {
  visible: boolean;
  gameTitle: string;
  isPaused: boolean;
  /** Emulation speed multiplier; 1.0 = real NTSC speed. */
  speed: number;
  /** Mono microtext shown under the title: resolution / backend info. */
  info: string;
  isFullscreen: boolean;
  onPauseResume: () => void;
  onSpeedChange: (value: number) => void;
  onQuickSave: () => void;
  onQuickLoad: () => void;
  onScreenshot: () => void;
  onMenu: () => void;
  onFullscreen: () => void;
  onExit: () => void;
  /** Deck hover/focus tracking so the auto-hide timer never fires while the
   * user is interacting with (or keyboard-focused inside) the deck. */
  onActiveChange: (active: boolean) => void;
}

/** Icon button used across the deck. The `accent` tint follows the Super
 * Famicom face-button palette (see index.css tokens) and only shows on
 * hover/focus so the resting deck stays quiet. */
function DeckButton({
  label,
  accent,
  onClick,
  children,
  emphasis = false,
}: {
  label: string;
  accent?: 'red' | 'yellow' | 'green' | 'blue';
  onClick: () => void;
  children: ReactNode;
  emphasis?: boolean;
}) {
  const accentClass = accent ? ` deck-btn--${accent}` : '';
  return (
    <button
      type="button"
      className={`deck-btn${emphasis ? ' deck-btn--emphasis' : ''}${accentClass}`}
      onClick={(e) => {
        // A mouse click leaves the button focused, and the next in-game
        // Enter (Start) or Space press would re-activate it. Drop focus for
        // pointer clicks; keyboard activation (detail === 0) keeps focus so
        // tab-navigation still works.
        if (e.detail > 0) {
          e.currentTarget.blur();
        }
        onClick();
      }}
      title={label}
      aria-label={label}
    >
      {children}
    </button>
  );
}

function Divider() {
  return <div className="deck-divider" aria-hidden />;
}

export function ControlDeck({
  visible,
  gameTitle,
  isPaused,
  speed,
  info,
  isFullscreen,
  onPauseResume,
  onSpeedChange,
  onQuickSave,
  onQuickLoad,
  onScreenshot,
  onMenu,
  onFullscreen,
  onExit,
  onActiveChange,
}: ControlDeckProps) {
  const handlePointerEnter = (_e: PointerEvent) => onActiveChange(true);
  const handlePointerLeave = (_e: PointerEvent) => onActiveChange(false);
  const handleFocus = (_e: FocusEvent) => onActiveChange(true);
  const handleBlur = (e: FocusEvent<HTMLDivElement>) => {
    // Only release when focus leaves the deck entirely, not when it moves
    // between deck buttons.
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
      onActiveChange(false);
    }
  };

  return (
    <div
      className={`absolute inset-x-0 bottom-0 z-30 flex justify-center px-4 pb-4 pointer-events-none`}
    >
      <div
        role="toolbar"
        aria-label="Emulator controls"
        className={`control-deck pointer-events-auto ${visible ? 'control-deck--visible' : 'control-deck--hidden'}`}
        onPointerEnter={handlePointerEnter}
        onPointerLeave={handlePointerLeave}
        onFocusCapture={handleFocus}
        onBlurCapture={handleBlur}
      >
        {/* Game identity + state */}
        <div className="flex min-w-0 flex-col gap-0.5 pr-1">
          <div className="flex items-center gap-2 min-w-0">
            <span
              className={`deck-status-dot ${isPaused ? 'deck-status-dot--paused' : 'deck-status-dot--running'}`}
              aria-hidden
            />
            <span className="truncate text-sm font-semibold max-w-[14rem]">
              {gameTitle}
            </span>
          </div>
          <span className="deck-info font-mono">{info}</span>
        </div>

        <Divider />

        {/* Transport: pause/resume + speed */}
        <DeckButton
          label={isPaused ? 'Resume (Space)' : 'Pause (Space)'}
          accent="blue"
          emphasis
          onClick={onPauseResume}
        >
          {isPaused ? <IconPlay size={20} /> : <IconPause size={20} />}
        </DeckButton>

        <div className="deck-speed" role="group" aria-label="Emulation speed">
          <button
            type="button"
            className="deck-speed-step"
            onClick={() => onSpeedChange(speed - 0.05)}
            title="Slower (−0.05×)"
            aria-label="Slower"
          >
            <IconMinus size={14} />
          </button>
          <button
            type="button"
            className={`deck-speed-value font-mono ${speed !== 1 ? 'deck-speed-value--modified' : ''}`}
            onClick={() => onSpeedChange(1.0)}
            title="Reset speed to 1.00×"
          >
            {speed.toFixed(2)}×
          </button>
          <button
            type="button"
            className="deck-speed-step"
            onClick={() => onSpeedChange(speed + 0.05)}
            title="Faster (+0.05×)"
            aria-label="Faster"
          >
            <IconPlus size={14} />
          </button>
        </div>

        <Divider />

        {/* Quick actions */}
        <DeckButton label="Quick save (F5)" accent="green" onClick={onQuickSave}>
          <IconSave />
        </DeckButton>
        <DeckButton label="Quick load (F9)" accent="yellow" onClick={onQuickLoad}>
          <IconLoad />
        </DeckButton>
        <DeckButton label="Screenshot (F8)" onClick={onScreenshot}>
          <IconCamera />
        </DeckButton>
        <DeckButton label="Quick menu (Esc)" onClick={onMenu}>
          <IconGrid />
        </DeckButton>

        <Divider />

        <DeckButton
          label={isFullscreen ? 'Exit fullscreen (F11)' : 'Fullscreen (F11)'}
          onClick={onFullscreen}
        >
          {isFullscreen ? <IconContract /> : <IconExpand />}
        </DeckButton>
        <DeckButton label="Exit game" accent="red" onClick={onExit}>
          <IconPower />
        </DeckButton>
      </div>
    </div>
  );
}
