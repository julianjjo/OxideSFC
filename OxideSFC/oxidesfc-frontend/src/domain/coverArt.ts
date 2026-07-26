import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import type { Game } from '../stores/libraryStore';

/**
 * Turning a cover image on disk into something the webview will render.
 *
 * This is the part that was quietly broken before: components rendered
 * `<img src={game.custom_cover_path} />` with a raw filesystem path, which a
 * webview refuses to load — so cover art could never have appeared even if
 * something had been writing the field (nothing was). Local files have to go
 * through Tauri's asset protocol, which is enabled in `tauri.conf.json` with its
 * scope deliberately narrowed to the covers directory alone; a broad scope would
 * hand the webview read access to arbitrary files.
 */

/** Absolute path of the covers directory, fetched once and reused. */
let coversDirPromise: Promise<string> | null = null;

export function getCoversDir(): Promise<string> {
  // Cached as the promise rather than the value so concurrent callers during
  // startup share one round-trip instead of racing several.
  coversDirPromise ??= invoke<string>('get_covers_dir').catch((error) => {
    console.error('Failed to resolve covers directory:', error);
    coversDirPromise = null;
    throw error;
  });
  return coversDirPromise;
}

/** Join a directory and file name with the platform separator already in `dir`. */
function joinPath(dir: string, file: string): string {
  const separator = dir.includes('\\') ? '\\' : '/';
  return dir.endsWith(separator) ? `${dir}${file}` : `${dir}${separator}${file}`;
}

/**
 * A renderable `src` for this game's cover, or null when it has none.
 *
 * `coversDir` comes from `getCoversDir()`; it is passed in rather than awaited
 * here so this stays synchronous and usable directly in render.
 */
export function coverSrc(game: Game, coversDir: string | null): string | null {
  if (game.cover_file && coversDir) {
    return convertFileSrc(joinPath(coversDir, game.cover_file));
  }
  // A path the user pointed at directly, wherever it lives. Outside the asset
  // scope it will not load, so this is a best effort until a "choose your own
  // image" flow exists to copy the file into the covers directory.
  if (game.custom_cover_path) {
    return convertFileSrc(game.custom_cover_path);
  }
  return null;
}

/** Where a resolved cover came from, mirroring `CoverSource` in covers.rs. */
export type CoverSource = 'cache' | 'local' | 'libretro' | 'missing' | 'unavailable';

export interface CoverResult {
  game_id: string;
  path: string | null;
  file: string | null;
  source: CoverSource;
}

export interface FetchCoversProgress {
  done: number;
  total: number;
  found: number;
  /** Title currently being looked up, for the progress line. */
  current: string;
}

/** How many lookups run at once. */
const CONCURRENCY = 5;

/**
 * Resolve covers for a set of games.
 *
 * Concurrency lives here rather than in Rust so that cancelling is simply
 * "stop queueing", progress is exact without an event channel, and each backend
 * call stays a small independent unit that is safe to retry.
 *
 * Returns the results in completion order. Individual failures are folded into an
 * `unavailable` result rather than rejecting: one unreachable image must not
 * abandon the rest of the library.
 */
export async function fetchCovers(
  games: Game[],
  options: {
    allowDownload: boolean;
    force?: boolean;
    onProgress?: (progress: FetchCoversProgress) => void;
    shouldStop?: () => boolean;
  }
): Promise<CoverResult[]> {
  const { allowDownload, force = false, onProgress, shouldStop } = options;
  const results: CoverResult[] = [];
  let cursor = 0;
  let done = 0;
  let found = 0;

  const worker = async () => {
    for (;;) {
      if (shouldStop?.()) return;
      const index = cursor++;
      if (index >= games.length) return;
      const game = games[index];

      try {
        const result = await invoke<CoverResult>('fetch_cover', {
          gameId: game.id,
          allowDownload,
          force,
        });
        results.push(result);
        if (result.file) found++;
      } catch (error) {
        console.error(`Cover lookup failed for ${game.title}:`, error);
        results.push({
          game_id: game.id,
          path: null,
          file: null,
          source: 'unavailable',
        });
      }

      done++;
      onProgress?.({ done, total: games.length, found, current: game.title });
    }
  };

  await Promise.all(
    Array.from({ length: Math.min(CONCURRENCY, games.length) }, () => worker())
  );

  return results;
}

/** Games that have no cover yet. */
export function gamesNeedingCovers(games: Game[]): Game[] {
  return games.filter((game) => !game.cover_file && !game.custom_cover_path);
}
