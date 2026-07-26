import type { DragEvent } from 'react';
import type { Game, LibrarySortKey } from '../../stores/libraryStore';
import { cartToneClass } from '../../domain/cartTone';
import {
  displayTitle,
  formatLastPlayed,
  formatRomSize,
  regionTag,
} from '../../domain/romFormat';
import { IconPlaySolid, IconStar, IconSortAsc, IconSortDesc } from '../common/icons';

interface GameListProps {
  games: Game[];
  sortBy: LibrarySortKey;
  sortOrder: 'asc' | 'desc';
  onSort: (key: LibrarySortKey) => void;
  onPlay: (game: Game) => void;
  onDetails: (game: Game) => void;
  onToggleFavorite: (game: Game) => void;
}

interface Column {
  key: LibrarySortKey | null;
  label: string;
  /** Screen-reader name, for columns whose header is a glyph. */
  srLabel?: string;
  /** Rendered instead of `label` when the header is not a word. */
  glyph?: React.ReactNode;
  className?: string;
}

const COLUMNS: Column[] = [
  // The favourite column's header is a star rather than the word: it sits above
  // a column of stars, and an empty string here left a zero-width sort button
  // that could be tabbed to but not seen or clicked.
  {
    key: 'favorite',
    label: '',
    srLabel: 'Favourite',
    glyph: <IconStar size={12} />,
    className: 'w-9',
  },
  { key: 'title', label: 'Title' },
  { key: null, label: 'Region', className: 'w-28' },
  { key: null, label: 'Size', className: 'w-24' },
  { key: 'play_count', label: 'Plays', className: 'w-20' },
  { key: 'last_played', label: 'Last played', className: 'w-32' },
  { key: null, label: '', className: 'w-20' },
];

/**
 * Dense, sortable table of the library.
 *
 * Grid mode is for browsing by recognition; this is for finding a title you
 * already have in mind, or comparing dumps of the same game. It used to be the
 * same cards from the grid stacked one per row, which was strictly worse than
 * the grid at both jobs -- one game per screen-height, no comparable columns,
 * and no way to sort by anything you could see.
 */
export function GameList({
  games,
  sortBy,
  sortOrder,
  onSort,
  onPlay,
  onDetails,
  onToggleFavorite,
}: GameListProps) {
  const handleDragStart = (e: DragEvent, gameId: string) => {
    e.dataTransfer.setData('gameId', gameId);
    e.dataTransfer.effectAllowed = 'copy';
  };

  return (
    <table className="rom-table">
      <thead>
        <tr>
          {COLUMNS.map((column, index) => (
            <th key={`${column.label}-${index}`} className={column.className}>
              {column.key ? (
                <button
                  type="button"
                  onClick={() => onSort(column.key!)}
                  aria-label={`Sort by ${column.srLabel || column.label}`}
                  title={`Sort by ${column.srLabel || column.label}`}
                >
                  {column.glyph ?? column.label}
                  {sortBy === column.key &&
                    (sortOrder === 'asc' ? <IconSortAsc /> : <IconSortDesc />)}
                </button>
              ) : (
                column.label
              )}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {games.map((game) => {
          const title = displayTitle(game.title);
          return (
            <tr
              key={game.id}
              draggable
              onDragStart={(e) => handleDragStart(e, game.id)}
              onDoubleClick={() => onPlay(game)}
            >
              <td>
                <button
                  type="button"
                  onClick={() => onToggleFavorite(game)}
                  className={`transition-colors ${
                    game.favorite ? 'text-sfc-yellow' : 'text-mute hover:text-ink'
                  }`}
                  aria-pressed={game.favorite}
                  aria-label={
                    game.favorite
                      ? `Remove ${title} from favourites`
                      : `Add ${title} to favourites`
                  }
                  title={game.favorite ? 'Remove from favourites' : 'Add to favourites'}
                >
                  <IconStar size={14} filled={game.favorite} />
                </button>
              </td>

              <td className="max-w-0">
                <div className="flex items-center gap-2.5">
                  {/* The card tone, carried into the row so a game reads the
                      same in both views. */}
                  <span className={`rom-swatch ${cartToneClass(game.title)}`} aria-hidden />
                  <button
                    type="button"
                    onClick={() => onDetails(game)}
                    className="min-w-0 truncate text-left font-semibold text-ink hover:underline"
                    title={game.file_name}
                  >
                    {title}
                  </button>
                </div>
              </td>

              <td className="register">{regionTag(game.country)}</td>
              <td className="register">{formatRomSize(game.file_size)}</td>
              <td className="register">{game.play_count || '—'}</td>
              <td className="register">{formatLastPlayed(game.last_played)}</td>

              <td className="text-right">
                <button
                  type="button"
                  onClick={() => onPlay(game)}
                  className="btn btn--secondary h-7 px-2.5 text-[0.75rem]"
                  aria-label={`Play ${title}`}
                >
                  <IconPlaySolid size={11} />
                  Play
                </button>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
