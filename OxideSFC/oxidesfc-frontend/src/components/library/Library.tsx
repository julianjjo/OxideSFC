import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
    setSortOrder,
    setViewMode,
    toggleSort,
  } = useLibraryStore();

  const { settings } = useSettingsStore();
  const { loadRom, start } = useEmulationStore();

  // The open game is held by id, not as an object. Holding the object froze a
  // snapshot taken when the modal opened: `toggleFavorite` maps a *new* Game into
  // the store, so the modal's own favourite button never reflected its own click
  // (and a second click, the natural response, silently undid the first). Looking
  // it up from the store each render also keeps a newly fetched cover in sync.
  const [selectedGameId, setSelectedGameId] = useState<string | null>(null);
  const selectedGame = useMemo(
    () => (selectedGameId ? games.find((g) => g.id === selectedGameId) ?? null : null),
    [selectedGameId, games]
  );
  const setSelectedGame = useCallback(
    (game: Game | null) => setSelectedGameId(game?.id ?? null),
    []
  );
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
   *
   * Cancellable and single-flight. A sweep can span hundreds of network lookups,
   * so it needs to stop when the user leaves the screen (otherwise it keeps
   * running and calls `setCoverProgress` on an unmounted tree) and it must not
   * overlap itself when a second folder is added mid-sweep.
   */
  const cancelCoverFetchRef = useRef(false);
  const coverFetchRunningRef = useRef(false);

  // Abort on unmount. Also the reason `shouldStop` is a ref rather than state:
  // the workers read it between every lookup and must see the latest value
  // without waiting for a re-render.
  useEffect(
    () => () => {
      cancelCoverFetchRef.current = true;
    },
    []
  );

  const runCoverFetch = useCallback(
    async (candidates: Game[]) => {
      if (candidates.length === 0 || coverFetchRunningRef.current) return;
      coverFetchRunningRef.current = true;
      cancelCoverFetchRef.current = false;
      setCoverProgress({ done: 0, total: candidates.length, found: 0, current: '' });
      try {
        await fetchCovers(candidates, {
          allowDownload: settings.library?.use_metadata !== false,
          onProgress: (progress) => {
            if (!cancelCoverFetchRef.current) setCoverProgress(progress);
          },
          shouldStop: () => cancelCoverFetchRef.current,
        });
        // One reload at the end rather than per image: repainting the whole shelf
        // on every arrival would thrash a large library for no benefit.
        if (!cancelCoverFetchRef.current) await loadGames();
      } catch (error) {
        console.error('Cover fetch failed:', error);
      } finally {
        coverFetchRunningRef.current = false;
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
          // The tie-break is pre-multiplied by `direction` so the `* direction`
          // applied to the result below cancels out and titles always read A-Z
          // within each group. It was `* -direction`, which squared to -1 and
          // made every favourite-sorted list run Z-A.
          comparison =
            (a.favorite ? 1 : 0) - (b.favorite ? 1 : 0) ||
            displayTitle(a.title).localeCompare(displayTitle(b.title)) * direction;
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
      const knownBefore = new Set(games.map((g) => g.id));
      const result = await scanDirectory(selected, settings.library?.scan_recursive !== false);
      setCountsKey((k) => k + 1);
      // Look up art for whatever the scan just *added*. `add_game_folder` returns
      // the whole merged library, not the delta, so filtering against the ids we
      // already had is what keeps this from re-sweeping the entire collection on
      // every folder add. Deliberately after the scan rather than inside it:
      // network lookups would make scanning slow and failure-prone, and the shelf
      // is already usable without covers.
      const added = result.games.filter((g) => !knownBefore.has(g.id));
      void runCoverFetch(gamesNeedingCovers(added));
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

  /**
   * Advance to the next sort column.
   *
   * Direction is a *separate* control beside it. Routing both through one button
   * meant `toggleSort` was always handed a different key, so its
   * "same column flips the direction" branch was unreachable and the arrow the
   * button rendered could never be changed -- in list mode you could at least
   * click a column header twice, but in grid mode the direction was frozen.
   */
  const cycleSortColumn = () => {
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
          <span className="flex flex-none items-center gap-1.5">
            <span className="chip chip--accent" role="status">
              Covers {coverProgress.done}/{coverProgress.total}
              {coverProgress.found > 0 ? ` · ${coverProgress.found} found` : ''}
            </span>
            <button
              type="button"
              onClick={() => {
                cancelCoverFetchRef.current = true;
              }}
              className="btn btn--ghost h-7 px-2 text-[0.75rem]"
            >
              Stop
            </button>
          </span>
        )}

        <div className="flex-1" />

        <div className="seg" role="group" aria-label="Sort">
          <button
            type="button"
            onClick={cycleSortColumn}
            className="seg-item"
            title="Sort by a different column"
          >
            {SORT_LABELS[sortBy]}
          </button>
          <button
            type="button"
            onClick={() => setSortOrder(sortOrder === 'asc' ? 'desc' : 'asc')}
            className="seg-item"
            title={sortOrder === 'asc' ? 'Ascending — click for descending' : 'Descending — click for ascending'}
            aria-label={`Sort direction: ${sortOrder === 'asc' ? 'ascending' : 'descending'}`}
          >
            {sortOrder === 'asc' ? <IconSortAsc /> : <IconSortDesc />}
          </button>
        </div>

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
              refreshKey={countsKey}
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
        onCollectionsChange={() => setCountsKey((k) => k + 1)}
      />
    </div>
  );
}
