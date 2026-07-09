import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '../common/Button';
import { Modal } from '../common/Modal';
import { Toggle } from '../common/Toggle';
import { useLibraryStore } from '../../stores/libraryStore';
import type { Game } from '../../stores/libraryStore';

// Re-exported so existing `import { Game } from './GameDetailsModal'` /
// `from './index'` call sites keep working. The real shape lives in
// libraryStore.ts, matching what the `get_games`/`scan_directory` Tauri
// commands actually return -- there is no separate crc32/region/developer/
// etc. metadata on the backend today (see GameMetadata in domain/types.ts
// for the *external* metadata shape, which is a different concept).
export type { Game };

interface GameDetailsModalProps {
  isOpen: boolean;
  onClose: () => void;
  game: Game | null;
  onPlay: (game: Game) => void;
  onEdit: (game: Game) => void;
  onDelete: (game: Game) => void;
  onManageSaves: (game: Game) => void;
  // Matches the loosely-typed `theme: string` used by GameGrid/GameCard/
  // Library (settings.general.theme is a plain string, not a literal union).
  theme?: string;
}

export function GameDetailsModal({
  isOpen,
  onClose,
  game,
  onPlay,
  onEdit,
  onDelete,
  onManageSaves,
  theme = 'dark',
}: GameDetailsModalProps) {
  const { toggleFavorite } = useLibraryStore();
  const [isFavorite, setIsFavorite] = useState(game?.favorite || false);
  const [playTime, setPlayTime] = useState<number | null>(null);
  // Tracks which game id the most recent async play-time/cover requests
  // were issued for, so a resolving request from a game the user has since
  // navigated away from (rapid switching between games in the grid) is
  // discarded instead of overwriting state for the *currently* displayed
  // game.
  const requestedGameIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (game) {
      setIsFavorite(game.favorite);
      requestedGameIdRef.current = game.id;
      loadPlayTime(game.id);
    } else {
      setPlayTime(null);
    }
  }, [game]);

  const loadPlayTime = async (gameId: string) => {
    try {
      const time = await invoke<number>('get_game_play_time', { gameId });
      if (requestedGameIdRef.current !== gameId) return; // stale: game changed since this request was issued
      setPlayTime(time);
    } catch (error) {
      console.error('Failed to load play time:', error);
      if (requestedGameIdRef.current !== gameId) return;
      setPlayTime(null);
    }
  };

  const handleToggleFavorite = async () => {
    if (!game) return;
    try {
      // Route through libraryStore rather than calling invoke() directly so
      // the grid's sort-by-favorite reflects the change immediately, and
      // rely on the store's fresh return value (not a value captured in
      // this closure) so rapid double-toggles can't cancel each other out.
      const newValue = await toggleFavorite(game.id);
      setIsFavorite(newValue);
    } catch (error) {
      console.error('Failed to toggle favorite:', error);
    }
  };

  const formatFileSize = (bytes: number): string => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  };

  // `get_game_play_time` returns `total_play_seconds` (see
  // src-tauri/src/commands/library.rs's `Game.total_play_seconds`), not
  // minutes -- format from seconds into a human-readable duration.
  const formatPlayTime = (seconds: number | null): string => {
    if (seconds === null || seconds === 0) return 'No play time recorded';
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    if (hours > 0) {
      return `${hours}h ${mins}m`;
    }
    if (mins > 0) {
      return `${mins}m`;
    }
    return `${seconds}s`;
  };

  const formatDate = (dateString: string | null): string => {
    if (!dateString) return 'Never';
    const date = new Date(dateString);
    return date.toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  if (!game) return null;

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={game.title}
      size="lg"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
        </>
      }
    >
      <div className="flex flex-col md:flex-row gap-6">
        {/* Cover Image / Screenshots */}
        <div className="w-full md:w-1/3">
          <div className={`aspect-square rounded-lg overflow-hidden ${
            theme === 'light' ? 'bg-gray-200' : 'bg-slate-700'
          }`}>
            {/* No metadata-scraping backend exists yet to fetch cover art
                (get_game_cover has no matching Tauri command -- calling it
                would always reject). Only show a real image when the game
                has a locally-set custom_cover_path; otherwise fall back to
                the same static placeholder GameCard.tsx uses elsewhere in
                the library, for visual consistency. */}
            {game.custom_cover_path ? (
              <img
                src={game.custom_cover_path}
                alt={`${game.title} cover`}
                className="w-full h-full object-cover"
              />
            ) : (
              <div className="w-full h-full flex items-center justify-center">
                <span className="text-6xl">🎮</span>
              </div>
            )}
          </div>
          
          {/* Favorite Toggle */}
          <div className="mt-4 flex items-center justify-center">
            <Toggle
              checked={isFavorite}
              onChange={handleToggleFavorite}
              label={isFavorite ? '★ Favorited' : '☆ Add to Favorites'}
            />
          </div>
        </div>

        {/* Game Details */}
        <div className="flex-1 space-y-4">
          {/* Basic Info */}
          <div className={`rounded-lg p-4 ${theme === 'light' ? 'bg-gray-100' : 'bg-slate-700'}`}>
            <h3 className="font-semibold mb-3">Game Information</h3>
            <dl className="space-y-2 text-sm">
              <div className="flex justify-between">
                <dt className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>System</dt>
                <dd>SNES</dd>
              </div>
              <div className="flex justify-between">
                <dt className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>Region</dt>
                <dd className="capitalize">{game.country || 'Unknown'}</dd>
              </div>
              <div className="flex justify-between">
                <dt className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>File Size</dt>
                <dd>{formatFileSize(game.file_size)}</dd>
              </div>
              {game.sram_size > 0 && (
                <div className="flex justify-between">
                  <dt className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>SRAM Size</dt>
                  <dd>{formatFileSize(game.sram_size)}</dd>
                </div>
              )}
            </dl>
          </div>

          {/* Play Stats */}
          <div className={`rounded-lg p-4 ${theme === 'light' ? 'bg-gray-100' : 'bg-slate-700'}`}>
            <h3 className="font-semibold mb-3">Play Statistics</h3>
            <dl className="space-y-2 text-sm">
              <div className="flex justify-between">
                <dt className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>Play Time</dt>
                <dd>{formatPlayTime(playTime)}</dd>
              </div>
              <div className="flex justify-between">
                <dt className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>Play Count</dt>
                <dd>{game.play_count} times</dd>
              </div>
              <div className="flex justify-between">
                <dt className={theme === 'light' ? 'text-gray-600' : 'text-slate-400'}>Last Played</dt>
                <dd>{formatDate(game.last_played)}</dd>
              </div>
            </dl>
          </div>

          {/* Actions */}
          <div className="flex flex-wrap gap-2 pt-2">
            <Button variant="primary" onClick={() => onPlay(game)}>
              ▶ Play
            </Button>
            <Button variant="secondary" onClick={() => onEdit(game)}>
              ✎ Edit
            </Button>
            <Button variant="secondary" onClick={() => onManageSaves(game)}>
              💾 Manage Saves
            </Button>
            <Button variant="danger" onClick={() => onDelete(game)}>
              🗑 Delete
            </Button>
          </div>
        </div>
      </div>
    </Modal>
  );
}
