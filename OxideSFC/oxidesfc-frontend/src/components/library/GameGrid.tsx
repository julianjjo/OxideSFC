import { Game } from '../../stores/libraryStore';
import { GameCard } from './GameCard';

interface GameGridProps {
  games: Game[];
  onPlay: (game: Game) => void;
  onDetails?: (game: Game) => void;
  theme: string;
}

export function GameGrid({ games, onPlay, onDetails, theme }: GameGridProps) {
  return (
    <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 2xl:grid-cols-6 gap-4">
      {games.map(game => (
        <GameCard key={game.id} game={game} onPlay={onPlay} onDetails={onDetails} theme={theme} />
      ))}
    </div>
  );
}
