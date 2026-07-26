import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Game } from '../../stores/libraryStore';
import { cartToneClass } from '../../domain/cartTone';
import {
  displayTitle,
  formatLastPlayed,
  formatPlayTime,
  formatRomSize,
  regionTag,
} from '../../domain/romFormat';
import { coverSrc } from '../../domain/coverArt';
import { Button } from '../common/Button';
import { IconPlaySolid } from '../common/icons';

interface ContinueHeroProps {
  game: Game;
  onPlay: (game: Game) => void;
  onDetails: (game: Game) => void;
  coversDir?: string | null;
}

/**
 * The library's opening statement: the game you were last playing, with the
 * facts that decide whether to go back to it.
 *
 * A hero here is not decoration -- resuming the last game is by far the most
 * common reason to open an emulator, and it was previously buried somewhere in
 * an alphabetical grid. The stats shown are the ones that answer "where was I":
 * total time, session count, and how long ago.
 */
export function ContinueHero({
  game,
  onPlay,
  onDetails,
  coversDir = null,
}: ContinueHeroProps) {
  const [playSeconds, setPlaySeconds] = useState<number | null>(null);
  const title = displayTitle(game.title);
  const art = coverSrc(game, coversDir);
  const [artBroken, setArtBroken] = useState(false);
  useEffect(() => setArtBroken(false), [art]);

  useEffect(() => {
    let active = true;
    invoke<number>('get_game_play_time', { gameId: game.id })
      .then((seconds) => {
        // Discard a response for a game that is no longer the hero (the library
        // reloads after a scan, which can change which game is most recent).
        if (active) setPlaySeconds(seconds);
      })
      .catch(() => {
        if (active) setPlaySeconds(null);
      });
    return () => {
      active = false;
    };
  }, [game.id]);

  const stats: Array<[string, string]> = [
    ['Played', formatPlayTime(playSeconds)],
    ['Sessions', String(game.play_count || 0)],
    ['Last', formatLastPlayed(game.last_played)],
    ['Cartridge', `${regionTag(game.country)} · ${formatRomSize(game.file_size)}`],
  ];

  return (
    <section className="panel pinstripe-top overflow-hidden">
      <div className="flex flex-col gap-5 p-5 sm:flex-row sm:items-center">
        {/* The same cart proportion as the grid, just larger -- see
            --cart-aspect. Sharing the ratio keeps a game recognisable between the
            hero and the shelf, and matches the artwork so no letterboxing
            appears. */}
        <div
          className={`${cartToneClass(game.title)} relative w-52 flex-none overflow-hidden rounded-md border border-line`}
          style={{ background: 'var(--cart-tone)', aspectRatio: 'var(--cart-aspect)' }}
        >
          {art && !artBroken ? (
            <img
              src={art}
              alt=""
              className="h-full w-full object-cover"
              onError={() => setArtBroken(true)}
            />
          ) : (
            <span
              className="absolute inset-0 flex items-center px-3 text-[var(--cart-ink)]"
              style={{ textShadow: '0 1px 2px rgba(0,0,0,.3)' }}
            >
              <span className="display-md line-clamp-3">{title}</span>
            </span>
          )}
          <span
            className="absolute inset-x-0 bottom-0 h-2"
            style={{
              background:
                'repeating-linear-gradient(to bottom, rgba(0,0,0,.16) 0 1px, transparent 1px 3px)',
            }}
            aria-hidden
          />
        </div>

        <div className="min-w-0 flex-1">
          <p className="eyebrow">Continue</p>
          <h2 className="display-lg mt-1 truncate text-ink" title={game.title}>
            {title}
          </h2>

          <dl className="mt-3 flex flex-wrap gap-x-6 gap-y-1.5">
            {stats.map(([label, value]) => (
              <div key={label}>
                <dt className="microlabel">{label}</dt>
                <dd className="register mt-0.5 text-ink">{value}</dd>
              </div>
            ))}
          </dl>
        </div>

        <div className="flex flex-none gap-2">
          <Button size="lg" leftIcon={<IconPlaySolid size={15} />} onClick={() => onPlay(game)}>
            Resume
          </Button>
          <Button variant="secondary" size="lg" onClick={() => onDetails(game)}>
            Details
          </Button>
        </div>
      </div>
    </section>
  );
}
