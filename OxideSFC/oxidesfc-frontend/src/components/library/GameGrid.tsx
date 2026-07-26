import type { Game } from '../../stores/libraryStore';
import { GameCard } from './GameCard';

interface GameGridProps {
  games: Game[];
  onPlay: (game: Game) => void;
  onDetails?: (game: Game) => void;
  onToggleFavorite?: (game: Game) => void;
  coversDir?: string | null;
}

/**
 * Cap on the entrance stagger.
 *
 * The shelf's one orchestrated moment is cards rising in sequence, which reads
 * well for the first couple of rows and becomes a liability in a large library:
 * at 18ms per card, a 300-ROM shelf would still be animating five seconds after
 * it opened. Past this many cards the rest appear together.
 */
const STAGGER_LIMIT = 24;
const STAGGER_STEP_MS = 18;

export function GameGrid({
  games,
  onPlay,
  onDetails,
  onToggleFavorite,
  coversDir = null,
}: GameGridProps) {
  return (
    <div
      className="grid gap-3"
      // Auto-fill against a minimum rather than fixed breakpoint counts, so the
      // shelf reflows continuously as the window (or the sidebar) changes width
      // instead of jumping between five and six columns.
      //
      // The minimum is set for the card's landscape shape: at 148px (the value
      // used while the card was portrait) a 1.43 card is only ~103px tall, too
      // small to read box art or a title.
      style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(196px, 1fr))' }}
    >
      {games.map((game, index) => (
        <GameCard
          key={game.id}
          game={game}
          onPlay={onPlay}
          onDetails={onDetails}
          onToggleFavorite={onToggleFavorite}
          revealDelay={index < STAGGER_LIMIT ? index * STAGGER_STEP_MS : 0}
          coversDir={coversDir}
        />
      ))}
    </div>
  );
}
