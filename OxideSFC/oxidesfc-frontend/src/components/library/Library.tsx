import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { useLibraryStore, Game } from '../../stores/libraryStore';
import { useEmulationStore } from '../../stores/emulationStore';
import { useSettingsStore } from '../../stores/settingsStore';
import { GameCard } from './GameCard';
import { GameGrid } from './GameGrid';
import { GameDetailsModal } from './GameDetailsModal';

interface LibraryProps {
  onPlayGame: () => void;
}

export function Library({ onPlayGame }: LibraryProps) {
  const { settings } = useSettingsStore();
  const theme = settings.general.theme || 'dark';
  
  const {
    games,
    isLoading,
    isScanning,
    searchQuery,
    sortBy,
    sortOrder,
    viewMode,
    loadGames,
    scanDirectory,
    removeGame,
    setSearchQuery,
    setSortBy,
    setSortOrder,
    setViewMode,
  } = useLibraryStore();

  const { loadRom, start } = useEmulationStore();
  const [selectedGame, setSelectedGame] = useState<Game | null>(null);

  useEffect(() => {
    loadGames();
  }, [loadGames]);

  // Filter and sort games
  const filteredGames = games
    .filter(game =>
      game.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
      game.file_name.toLowerCase().includes(searchQuery.toLowerCase())
    )
    .sort((a, b) => {
      let comparison = 0;
      switch (sortBy) {
        case 'title':
          comparison = a.title.localeCompare(b.title);
          break;
        case 'last_played':
          comparison = (a.last_played || '').localeCompare(b.last_played || '');
          break;
        case 'play_count':
          comparison = a.play_count - b.play_count;
          break;
        case 'favorite':
          comparison = (a.favorite ? 1 : 0) - (b.favorite ? 1 : 0);
          break;
      }
      return sortOrder === 'asc' ? comparison : -comparison;
    });

  const handleAddFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select ROM Folder',
    });

    if (selected) {
      try {
        await scanDirectory(selected as string);
      } catch (error) {
        console.error('Failed to scan directory:', error);
      }
    }
  };

  const handlePlayGame = async (game: Game) => {
    try {
      await loadRom(game.file_path);
      await start(game.id);
      onPlayGame();
    } catch (error) {
      console.error('Failed to start game:', error);
    }
  };

  const handlePlayFromDetails = (game: Game) => {
    setSelectedGame(null);
    void handlePlayGame(game);
  };

  const handleEditGame = (game: Game) => {
    // No game-metadata editing UI/backend command exists yet; this is a
    // placeholder until that feature lands.
    console.warn('Editing game metadata is not yet implemented:', game.title);
  };

  const handleDeleteGame = async (game: Game) => {
    try {
      await removeGame(game.id);
      setSelectedGame(null);
    } catch (error) {
      console.error('Failed to delete game:', error);
    }
  };

  const handleManageSaves = (game: Game) => {
    // No per-game save-state management UI exists outside of the in-emulator
    // quick menu yet; this is a placeholder until that feature lands.
    console.warn('Managing saves from the library is not yet implemented:', game.title);
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-lg">Loading library...</div>
      </div>
    );
  }

  return (
    <div className={`h-full flex flex-col ${theme === 'light' ? 'bg-gray-100' : 'bg-slate-900'}`}>
      {/* Toolbar */}
      <div className={`flex items-center gap-4 p-4 ${theme === 'light' ? 'bg-white border-b border-gray-200' : 'bg-slate-800 border-b border-slate-700'}`}>
        {/* Search */}
        <div className="flex-1">
          <input
            type="text"
            placeholder="Search games..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className={`w-full px-4 py-2 rounded-lg ${
              theme === 'light'
                ? 'bg-gray-100 border border-gray-300 focus:border-primary-500'
                : 'bg-slate-700 border border-slate-600 focus:border-primary-500'
            } outline-none`}
          />
        </div>

        {/* Sort */}
        <select
          value={sortBy}
          onChange={(e) => setSortBy(e.target.value as typeof sortBy)}
          className={`px-3 py-2 rounded-lg ${
            theme === 'light'
              ? 'bg-gray-100 border border-gray-300'
              : 'bg-slate-700 border border-slate-600'
          } outline-none`}
        >
          <option value="title">Title</option>
          <option value="last_played">Last Played</option>
          <option value="play_count">Play Count</option>
          <option value="favorite">Favorite</option>
        </select>

        {/* Sort Order */}
        <button
          onClick={() => setSortOrder(sortOrder === 'asc' ? 'desc' : 'asc')}
          className={`p-2 rounded-lg ${
            theme === 'light'
              ? 'bg-gray-100 hover:bg-gray-200'
              : 'bg-slate-700 hover:bg-slate-600'
          }`}
        >
          {sortOrder === 'asc' ? '↑' : '↓'}
        </button>

        {/* View Mode */}
        <div className="flex gap-1">
          <button
            onClick={() => setViewMode('grid')}
            className={`p-2 rounded-lg ${
              viewMode === 'grid'
                ? 'bg-primary-600 text-white'
                : theme === 'light'
                ? 'bg-gray-100 hover:bg-gray-200'
                : 'bg-slate-700 hover:bg-slate-600'
            }`}
          >
            Grid
          </button>
          <button
            onClick={() => setViewMode('list')}
            className={`p-2 rounded-lg ${
              viewMode === 'list'
                ? 'bg-primary-600 text-white'
                : theme === 'light'
                ? 'bg-gray-100 hover:bg-gray-200'
                : 'bg-slate-700 hover:bg-slate-600'
            }`}
          >
            List
          </button>
        </div>

        {/* Add Folder */}
        <button
          onClick={handleAddFolder}
          disabled={isScanning}
          className="px-4 py-2 bg-primary-600 hover:bg-primary-700 disabled:bg-primary-800 rounded-lg flex items-center gap-2"
        >
          {isScanning ? 'Scanning...' : 'Add Folder'}
        </button>
      </div>

      {/* Games */}
      <div className="flex-1 overflow-auto p-4">
        {filteredGames.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-center">
            <div className="text-4xl mb-4">🎮</div>
            <h2 className="text-xl font-semibold mb-2">No games found</h2>
            <p className="text-gray-400 mb-4">Add a folder containing your SNES ROMs</p>
            <button
              onClick={handleAddFolder}
              className="px-4 py-2 bg-primary-600 hover:bg-primary-700 rounded-lg"
            >
              Add ROM Folder
            </button>
          </div>
        ) : viewMode === 'grid' ? (
          <GameGrid games={filteredGames} onPlay={handlePlayGame} onDetails={setSelectedGame} theme={theme} />
        ) : (
          <div className="space-y-2">
            {filteredGames.map(game => (
              <GameCard key={game.id} game={game} onPlay={handlePlayGame} onDetails={setSelectedGame} theme={theme} />
            ))}
          </div>
        )}
      </div>

      <GameDetailsModal
        isOpen={selectedGame !== null}
        onClose={() => setSelectedGame(null)}
        game={selectedGame}
        onPlay={handlePlayFromDetails}
        onEdit={handleEditGame}
        onDelete={(game) => void handleDeleteGame(game)}
        onManageSaves={handleManageSaves}
        theme={theme}
      />
    </div>
  );
}
