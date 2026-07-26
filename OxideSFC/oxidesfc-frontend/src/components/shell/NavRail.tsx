import {
  MarkSFC,
  IconLibrary,
  IconSettings,
  IconPlaySolid,
} from '../common/icons';

export type AppView = 'library' | 'settings' | 'emulator';

interface NavRailProps {
  view: AppView;
  onNavigate: (view: AppView) => void;
  /** Title of the loaded game, or null when nothing is running. */
  runningTitle: string | null;
  isPaused: boolean;
}

interface RailItem {
  view: Exclude<AppView, 'emulator'>;
  label: string;
  icon: React.ReactNode;
}

const ITEMS: RailItem[] = [
  { view: 'library', label: 'Library', icon: <IconLibrary /> },
  { view: 'settings', label: 'Settings', icon: <IconSettings /> },
];

/**
 * The app's primary navigation: a 60px icon rail rather than a header row.
 *
 * Two structural reasons, both about the library screen it sits beside. The
 * shelf gets the full window width, and "a game is loaded" becomes permanent
 * app state anchored at the foot of the rail instead of a `Back to Game` button
 * that materialised in the header only on some screens. A running game is the
 * single most important thing the app can be doing, so it should not be
 * something you can navigate away from and lose sight of.
 */
export function NavRail({ view, onNavigate, runningTitle, isPaused }: NavRailProps) {
  return (
    <nav className="rail" aria-label="Main">
      <div
        className="mb-1 flex h-10 w-10 items-center justify-center"
        title="OxideSFC"
      >
        <MarkSFC size={22} />
      </div>

      <div className="mb-1 h-px w-6 bg-line" aria-hidden />

      {ITEMS.map((item) => {
        const active = view === item.view;
        return (
          <button
            key={item.view}
            type="button"
            onClick={() => onNavigate(item.view)}
            className={`rail-btn ${active ? 'rail-btn--on' : ''}`}
            aria-current={active ? 'page' : undefined}
            title={item.label}
            aria-label={item.label}
          >
            {item.icon}
          </button>
        );
      })}

      <div className="flex-1" />

      {runningTitle && (
        <>
          <div className="mb-1 h-px w-6 bg-line" aria-hidden />
          <button
            type="button"
            onClick={() => onNavigate('emulator')}
            className={`rail-btn ${view === 'emulator' ? 'rail-btn--on' : ''}`}
            title={`${isPaused ? 'Paused' : 'Playing'}: ${runningTitle}`}
            aria-label={`Return to ${runningTitle}`}
          >
            <IconPlaySolid size={16} />
            {/* Status pip: green while the game is advancing, yellow while
                paused -- the same two colours the play deck's own status dot
                uses, so the reading carries over between screens. */}
            <span
              className="absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full"
              style={{
                background: isPaused ? 'var(--sfc-yellow)' : 'var(--sfc-green)',
              }}
              aria-hidden
            />
          </button>
        </>
      )}
    </nav>
  );
}
