import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useLibraryStore, type Game, type LibrarySortKey } from '../../stores/libraryStore';
import { useEmulationStore } from '../../stores/emulationStore';
import { useSettingsStore } from '../../stores/settingsStore';
import { Button } from '../common/Button';
import { Input } from '../common/Input';
import {
  IconSearch,
  IconGrid,
  IconList,
  IconPlus,
  IconSortAsc,
  IconSortDesc,
} from '../common/icons';
import { GameGrid } from './GameGrid';
import { GameList } from './GameList';
import { GameDetailsModal } from './GameDetailsModal';
import { ContinueHero } from './ContinueHero';
import { FilterSidebar, EMPTY_FILTERS, type FilterState } from './FilterSidebar';
import { CollectionFolders } from './CollectionFolders';
import { displayTitle } from '../../domain/romFormat';
import {
  fetchCovers,
  gamesNeedingCovers,
  getCoversDir,
  type FetchCoversProgress,
} from '../../domain/coverArt';

interface LibraryProps {
  onPlayGame: () => void;
}

const SORT_LABELS: Record<LibrarySortKey, string> = {
  title: 'Title',
  last_played: 'Last played',
  play_count: 'Most played',
  favorite: 'Favourites first',
};

/** Games played within this window count as "recently played". */
const RECENT_WINDOW_DAYS = 14;

export function Library({ onPlayGame }: LibraryProps) {
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
    toggleFavorite,
    setSearchQuery,
    setViewMode,
    toggleSort,
  } = useLibraryStore();

  const { settings } = useSettingsStore();
  const { loadRom, start } = useEmulationStore();

  const [selectedGame, setSelectedGame] = useState<Game | null>(null);
  const [filters, setFilters] = useState<FilterState>(EMPTY_FILTERS);
  const [collectionId, setCollectionId] = useState<string | null>(null);
  const [collectionGameIds, setCollectionGameIds] = useState<Set<string> | null>(null);
  const [countsKey, setCountsKey] = useState(0);
  const [coversDir, setCoversDir] = useState<string | null>(null);
  const [coverProgress, setCoverProgress] = useState<FetchCoversProgress | null>(null);

  useEffect(() => {
    void loadGames();
  }, [loadGames]);

  // The covers directory is needed to turn a stored file name into a renderable
  // asset-protocol URL. Resolved once; the helper caches the round-trip.
  useEffect(() => {
    getCoversDir().then(setCoversDir).catch(() => setCoversDir(null));
  }, []);

  /**
   * Resolve covers for games that lack one.
   *
   * `allowDownload` reflects the user's "Fetch metadata" preference, so the local
   * tier still runs for people who have opted out of network lookups.
   */
  const runCoverFetch = useCallback(
    async (candidates: Game[]) => {
      if (candidates.length === 0) return;
      setCoverProgress({ done: 0, total: candidates.length, found: 0, current: '' });
      try {
        await fetchCovers(candidates, {
          allowDownload: settings.library?.use_metadata !== false,
          onProgress: setCoverProgress,
        });
        // One reload at the end rather than per image: repainting the whole shelf
        // on every arrival would thrash a large library for no benefit.
        await loadGames();
      } catch (error) {
        console.error('Cover fetch failed:', error);
      } finally {
        setCoverProgress(null);
      }
    },
    [settings.library?.use_metadata, loadGames]
  );

  // Membership of the selected collection. Only the ids are kept and used to
  // filter the store's own list, so every card action (favourite, delete, play)
  // keeps operating on the same Game objects the rest of the app holds.
  useEffect(() => {
    if (!collectionId) {
      setCollectionGameIds(null);
      return;
    }
    let active = true;
    invoke<Array<{ id: string }>>('get_games_in_folder', { folderId: collectionId })
      .then((rows) => {
        if (active) setCollectionGameIds(new Set(rows.map((r) => r.id)));
      })
      .catch((error) => {
        console.error('Failed to load collection contents:', error);
        if (active) setCollectionGameIds(new Set());
      });
    return () => {
      active = false;
    };
  }, [collectionId, countsKey]);

  const favoriteCount = useMemo(() => games.filter((g) => g.favorite).length, [games]);

  const visibleGames = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    const recentCutoff = Date.now() - RECENT_WINDOW_DAYS * 86_400_000;

    const filtered = games.filter((game) => {
      if (collectionGameIds && !collectionGameIds.has(game.id)) return false;

      if (filters.quickView === 'favorites' && !game.favorite) return false;
      if (filters.quickView === 'recent') {
        if (!game.last_played) return false;
        const played = new Date(game.last_played).getTime();
        if (Number.isNaN(played) || played < recentCutoff) return false;
      }

      if (
        filters.regions.length > 0 &&
        !filters.regions.includes((game.country || '').toLowerCase())
      ) {
        return false;
      }

      if (query) {
        // Match the display title too, so searching "zelda" finds
        // "Legend of Zelda, The - A Link to the Past (USA)".
        const haystack = `${game.title} ${game.file_name}`.toLowerCase();
        if (!haystack.includes(query)) return false;
      }

      return true;
    });

    const direction = sortOrder === 'asc' ? 1 : -1;
    return filtered.sort((a, b) => {
      let comparison = 0;
      switch (sortBy) {
        case 'title':
          comparison = displayTitle(a.title).localeCompare(displayTitle(b.title));
          break;
        case 'last_played':
          // Never-played sorts last regardless of direction: an empty value is
          // "no data", not "oldest", and letting it compare as an empty string
          // pushed unplayed games to the top of a most-recent-first list.
          if (!a.last_played && !b.last_played) comparison = 0;
          else if (!a.last_played) return 1;
          else if (!b.last_played) return -1;
          else comparison = a.last_played.localeCompare(b.last_played);
          break;
        case 'play_count':
          comparison = a.play_count - b.play_count;
          break;
        case 'favorite':
          comparison =
            (a.favorite ? 1 : 0) - (b.favorite ? 1 : 0) ||
            displayTitle(a.title).localeCompare(displayTitle(b.title)) * -direction;
          break;
      }
      return comparison * direction;
    });
  }, [games, searchQuery, sortBy, sortOrder, filters, collectionGameIds]);

  /** The hero's game: most recently played. */
  const continueGame = useMemo(() => {
    let best: Game | null = null;
    let bestTime = -Infinity;
    for (const game of games) {
      if (!game.last_played) continue;
      const time = new Date(game.last_played).getTime();
      if (!Number.isNaN(time) && time > bestTime) {
        bestTime = time;
        best = game;
      }
    }
    return best;
  }, [games]);

  // The hero is an entry point, not a search result: showing it above a filtered
  // shelf would put a game there that the current filters exclude.
  const browsing =
    !searchQuery.trim() &&
    filters.quickView === 'all' &&
    filters.regions.length === 0 &&
    !collectionId;

  const handlePlay = useCallback(
    async (game: Game) => {
      try {
        await loadRom(game.file_path);
        await start(game.id);
        onPlayGame();
      } catch (error) {
        console.error('Failed to start game:', error);
      }
    },
    [loadRom, start, onPlayGame]
  );

  const handleAddFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select ROM folder',
    });
    if (typeof selected !== 'string') return;
    try {
      const result = await scanDirectory(selected, settings.library?.scan_recursive !== false);
      setCountsKey((k) => k + 1);
      // Look up art for whatever the scan just added. Deliberately after the scan
      // rather than inside it: network lookups would make scanning slow and
      // failure-prone, and the shelf is already usable without covers.
      void runCoverFetch(gamesNeedingCovers(result.games));
    } catch (error) {
      console.error('Failed to scan folder:', error);
    }
  };

  const handleToggleFavorite = useCallback(
    async (game: Game) => {
      try {
        await toggleFavorite(game.id);
      } catch (error) {
        console.error('Failed to update favourite:', error);
      }
    },
    [toggleFavorite]
  );

  const handleDelete = async (game: Game) => {
    try {
      await removeGame(game.id);
      setSelectedGame(null);
      setCountsKey((k) => k + 1);
    } catch (error) {
      console.error('Failed to remove game:', error);
    }
  };

  const cycleSort = () => {
    const order: LibrarySortKey[] = ['title', 'last_played', 'play_count', 'favorite'];
    toggleSort(order[(order.indexOf(sortBy) + 1) % order.length]);
  };

  if (isLoading && games.length === 0) {
    return (
      <div className="flex h-full items-center justify-center">
        <p className="register">loading library…</p>
      </div>
    );
  }

  const libraryIsEmpty = games.length === 0;

  return (
    <div className="flex h-full flex-col">
      {/* Toolbar ---------------------------------------------------------- */}
      <div className="flex flex-none items-center gap-3 border-b border-line bg-panel px-4 py-3">
        <div className="w-full max-w-xs">
          <Input
            inputSize="sm"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search library"
            aria-label="Search library"
            leftIcon={<IconSearch size={15} />}
            onKeyDown={(e) => e.key === 'Escape' && setSearchQuery('')}
          />
        </div>

        <span className="register flex-none">
          {visibleGames.length}
          {visibleGames.length !== games.length ? ` / ${games.length}` : ''}{' '}
          {games.length === 1 ? 'game' : 'games'}
        </span>

        {coverProgress && (
          <span className="chip chip--accent flex-none" role="status">
            Covers {coverProgress.done}/{coverProgress.total}
            {coverProgress.found > 0 ? ` · ${coverProgress.found} found` : ''}
          </span>
        )}

        <div className="flex-1" />

        <button
          type="button"
          onClick={cycleSort}
          className="btn btn--secondary h-8 px-3 text-[0.8125rem]"
          title="Change sort order"
        >
          {SORT_LABELS[sortBy]}
          {sortOrder === 'asc' ? <IconSortAsc /> : <IconSortDesc />}
        </button>

        <div className="seg" role="group" aria-label="View mode">
          <button
            type="button"
            onClick={() => setViewMode('grid')}
            className={`seg-item ${viewMode === 'grid' ? 'seg-item--on' : ''}`}
            aria-pressed={viewMode === 'grid'}
            title="Grid"
            aria-label="Grid view"
          >
            <IconGrid size={15} />
          </button>
          <button
            type="button"
            onClick={() => setViewMode('list')}
            className={`seg-item ${viewMode === 'list' ? 'seg-item--on' : ''}`}
            aria-pressed={viewMode === 'list'}
            title="List"
            aria-label="List view"
          >
            <IconList size={15} />
          </button>
        </div>

        <Button
          size="sm"
          leftIcon={<IconPlus size={15} />}
          onClick={handleAddFolder}
          isLoading={isScanning}
          disabled={isScanning}
        >
          Add folder
        </Button>
      </div>

      <div className="flex min-h-0 flex-1">
        {/* Facets ------------------------------------------------------- */}
        {!libraryIsEmpty && (
          <aside className="w-52 flex-none space-y-5 overflow-y-auto border-r border-line px-3 py-4">
            <FilterSidebar
              filters={filters}
              onChange={setFilters}
              totalCount={games.length}
              favoriteCount={favoriteCount}
              refreshKey={countsKey}
            />
            <div className="h-px bg-line" aria-hidden />
            <CollectionFolders
              selectedId={collectionId}
              onSelect={setCollectionId}
              onGameFiled={() => setCountsKey((k) => k + 1)}
            />
          </aside>
        )}

        {/* Shelf -------------------------------------------------------- */}
        <div className="min-w-0 flex-1 overflow-y-auto">
          {libraryIsEmpty ? (
            <div className="flex h-full flex-col items-center justify-center px-6 text-center">
              <div className="panel pinstripe-top max-w-md px-8 py-10">
                <p className="eyebrow">No cartridges</p>
                <h2 className="display-lg mt-2 text-ink">Your shelf is empty</h2>
                <p className="mt-2 text-sm leading-relaxed text-dim">
                  Point OxideSFC at a folder of ROMs. Files stay where they are —
                  the library only records where to find them, and reads each
                  cartridge header for its region and size.
                </p>
                <div className="mt-5 flex justify-center">
                  <Button
                    leftIcon={<IconPlus size={15} />}
                    onClick={handleAddFolder}
                    isLoading={isScanning}
                  >
                    Add ROM folder
                  </Button>
                </div>
              </div>
            </div>
          ) : (
            <div className="space-y-4 p-4">
              {browsing && continueGame && (
                <ContinueHero
                  game={continueGame}
                  onPlay={handlePlay}
                  onDetails={setSelectedGame}
                  coversDir={coversDir}
                />
              )}

              {visibleGames.length === 0 ? (
                <div className="panel px-6 py-10 text-center">
                  <p className="display-md text-ink">Nothing matches</p>
                  <p className="mx-auto mt-2 max-w-sm text-sm leading-relaxed text-dim">
                    {searchQuery.trim()
                      ? `No game in your library matches “${searchQuery.trim()}”.`
                      : 'No game matches the filters you have selected.'}
                  </p>
                  <div className="mt-4 flex justify-center gap-2">
                    {searchQuery.trim() && (
                      <Button variant="secondary" size="sm" onClick={() => setSearchQuery('')}>
                        Clear search
                      </Button>
                    )}
                    {(filters.quickView !== 'all' ||
                      filters.regions.length > 0 ||
                      collectionId) && (
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => {
                          setFilters(EMPTY_FILTERS);
                          setCollectionId(null);
                        }}
                      >
                        Clear filters
                      </Button>
                    )}
                  </div>
                </div>
              ) : viewMode === 'grid' ? (
                <GameGrid
                  games={visibleGames}
                  onPlay={handlePlay}
                  onDetails={setSelectedGame}
                  onToggleFavorite={handleToggleFavorite}
                  coversDir={coversDir}
                />
              ) : (
                <GameList
                  games={visibleGames}
                  sortBy={sortBy}
                  sortOrder={sortOrder}
                  onSort={toggleSort}
                  onPlay={handlePlay}
                  onDetails={setSelectedGame}
                  onToggleFavorite={handleToggleFavorite}
                />
              )}
            </div>
          )}
        </div>
      </div>

      <GameDetailsModal
        isOpen={selectedGame !== null}
        onClose={() => setSelectedGame(null)}
        game={selectedGame}
        onPlay={(game) => {
          setSelectedGame(null);
          void handlePlay(game);
        }}
        onToggleFavorite={handleToggleFavorite}
        onDelete={(game) => void handleDelete(game)}
        coversDir={coversDir}
      />
    </div>
  );
}
