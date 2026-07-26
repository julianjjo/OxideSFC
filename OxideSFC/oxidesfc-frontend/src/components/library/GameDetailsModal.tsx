import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '../common/Button';
import { Modal, ConfirmModal } from '../common/Modal';
import { IconPlaySolid, IconStar, IconTrash } from '../common/icons';
import type { Game } from '../../stores/libraryStore';
import { cartToneClass } from '../../domain/cartTone';
import {
  displayTitle,
  formatDateTime,
  formatPlayTime,
  formatRomSize,
  formatSramSize,
  regionTag,
} from '../../domain/romFormat';
import { coverSrc } from '../../domain/coverArt';

// Re-exported so existing `import { Game } from './GameDetailsModal'` call sites
// keep working. The real shape lives in libraryStore.ts, matching what the
// `get_games` command actually returns.
export type { Game };

interface GameDetailsModalProps {
  isOpen: boolean;
  onClose: () => void;
  game: Game | null;
  onPlay: (game: Game) => void;
  onToggleFavorite: (game: Game) => void;
  onDelete: (game: Game) => void;
  coversDir?: string | null;
}

export function GameDetailsModal({
  isOpen,
  onClose,
  game,
  onPlay,
  onToggleFavorite,
  onDelete,
  coversDir = null,
}: GameDetailsModalProps) {
  const [playSeconds, setPlaySeconds] = useState<number | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  // Tracks which game the in-flight play-time request belongs to, so a response
  // for a game the user has since navigated away from is discarded instead of
  // overwriting the current one.
  const requestedIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (!game) {
      setPlaySeconds(null);
      return;
    }
    requestedIdRef.current = game.id;
    invoke<number>('get_game_play_time', { gameId: game.id })
      .then((seconds) => {
        if (requestedIdRef.current === game.id) setPlaySeconds(seconds);
      })
      .catch((error) => {
        console.error('Failed to load play time:', error);
        if (requestedIdRef.current === game.id) setPlaySeconds(null);
      });
  }, [game]);

  if (!game) return null;

  const title = displayTitle(game.title);
  const sram = formatSramSize(game.sram_size);
  const art = coverSrc(game, coversDir);

  const cartridge: Array<[string, string]> = [
    ['Region', `${game.country || 'Unknown'} · ${regionTag(game.country)}`],
    ['ROM size', formatRomSize(game.file_size)],
    ['Mapper', game.rom_type || 'Unknown'],
    // A cart either has battery-backed save memory or it does not; "0 Kbit" is
    // less informative than saying so.
    ['Save memory', sram ? `${sram} SRAM` : 'None (no battery save)'],
  ];

  const history: Array<[string, string]> = [
    ['Time played', formatPlayTime(playSeconds)],
    ['Sessions', String(game.play_count || 0)],
    ['Last played', formatDateTime(game.last_played)],
    ['Added', formatDateTime(game.created_at)],
  ];

  return (
    <>
      <Modal
        isOpen={isOpen}
        onClose={onClose}
        title={title}
        subtitle={game.file_name}
        size="lg"
        footer={
          <>
            <Button
              variant="ghost"
              size="sm"
              leftIcon={<IconTrash size={14} />}
              onClick={() => setConfirmDelete(true)}
              className="mr-auto"
            >
              Remove from library
            </Button>
            <Button variant="secondary" onClick={onClose}>
              Close
            </Button>
            <Button leftIcon={<IconPlaySolid size={14} />} onClick={() => onPlay(game)}>
              Play
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-5 sm:flex-row">
          <div className="w-full flex-none sm:w-56">
            <div
              className={`cart ${cartToneClass(game.title)} pointer-events-none`}
            >
              <div className={`cart-label${art ? ' cart-label--art' : ''}`}>
                {art ? (
                  <img src={art} alt="" className="cart-art" />
                ) : (
                  <h3 className="cart-title">{title}</h3>
                )}
              </div>
              <div className="cart-foot">
                <span className="register truncate">
                  {regionTag(game.country)} · {formatRomSize(game.file_size)}
                </span>
              </div>
            </div>

            <Button
              variant={game.favorite ? 'primary' : 'secondary'}
              size="sm"
              block
              className="mt-2.5"
              leftIcon={<IconStar size={14} filled={game.favorite} />}
              onClick={() => onToggleFavorite(game)}
            >
              {game.favorite ? 'Favourited' : 'Add to favourites'}
            </Button>
          </div>

          <div className="min-w-0 flex-1 space-y-4">
            <section>
              <p className="eyebrow mb-1.5">Cartridge</p>
              <dl className="overflow-hidden rounded-md border border-line">
                {cartridge.map(([label, value]) => (
                  <div
                    key={label}
                    className="flex items-center justify-between gap-4 border-b border-line px-3 py-2 last:border-b-0"
                  >
                    <dt className="text-[0.8125rem] text-mute">{label}</dt>
                    <dd className="register text-right text-ink">{value}</dd>
                  </div>
                ))}
              </dl>
            </section>

            <section>
              <p className="eyebrow mb-1.5">History</p>
              <dl className="overflow-hidden rounded-md border border-line">
                {history.map(([label, value]) => (
                  <div
                    key={label}
                    className="flex items-center justify-between gap-4 border-b border-line px-3 py-2 last:border-b-0"
                  >
                    <dt className="text-[0.8125rem] text-mute">{label}</dt>
                    <dd className="register text-right text-ink">{value}</dd>
                  </div>
                ))}
              </dl>
            </section>

            <section>
              <p className="eyebrow mb-1.5">File</p>
              <code className="block break-all rounded-md border border-line bg-raised px-3 py-2 font-mono text-[0.75rem] text-dim">
                {game.file_path}
              </code>
            </section>
          </div>
        </div>

        {/*
          There used to be "Edit" and "Manage saves" buttons here. Neither had an
          implementation behind it -- both handlers logged a "not yet
          implemented" warning and returned -- so they are gone rather than
          restyled. Save states are reachable from the in-game quick menu, which
          is where they actually work.
        */}
      </Modal>

      <ConfirmModal
        isOpen={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        onConfirm={() => {
          setConfirmDelete(false);
          onDelete(game);
        }}
        title={`Remove “${title}”?`}
        message="The entry leaves your library along with its play history. The ROM file on disk is not deleted, and a rescan will find it again."
        confirmText="Remove entry"
        variant="danger"
      />
    </>
  );
}
