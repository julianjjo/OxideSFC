import { Game } from '../../stores/libraryStore';

interface GameCardProps {
  game: Game;
  onPlay: (game: Game) => void;
  onDetails?: (game: Game) => void;
  theme: string;
}

export function GameCard({ game, onPlay, onDetails, theme }: GameCardProps) {
  const formatFileSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <div
      className={`rounded-lg overflow-hidden cursor-pointer transition-all hover:scale-105 ${
        theme === 'light'
          ? 'bg-white border border-gray-200 hover:shadow-lg'
          : 'bg-slate-800 border border-slate-700 hover:shadow-xl'
      }`}
    >
      {/* Cover / Placeholder */}
      <div className={`aspect-video flex items-center justify-center ${
        theme === 'light' ? 'bg-gray-200' : 'bg-slate-700'
      }`}>
        {game.custom_cover_path ? (
          <img src={game.custom_cover_path} alt={game.title} className="w-full h-full object-cover" />
        ) : (
          <span className="text-4xl">🎮</span>
        )}
      </div>

      {/* Info */}
      <div className="p-3">
        <h3 className="font-semibold truncate">{game.title}</h3>
        <p className={`text-sm ${theme === 'light' ? 'text-gray-500' : 'text-slate-400'}`}>
          {game.country} • {formatFileSize(game.file_size)}
        </p>

        {/* Play / Details Buttons */}
        <div className="mt-3 flex gap-2">
          <button
            onClick={() => onPlay(game)}
            className="flex-1 py-2 bg-primary-600 hover:bg-primary-700 rounded text-sm font-medium transition-colors"
          >
            Play
          </button>
          {onDetails && (
            <button
              onClick={() => onDetails(game)}
              aria-label={`Details for ${game.title}`}
              title="Details"
              className={`px-3 py-2 rounded text-sm font-medium transition-colors ${
                theme === 'light'
                  ? 'bg-gray-200 hover:bg-gray-300'
                  : 'bg-slate-700 hover:bg-slate-600'
              }`}
            >
              ℹ
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
