import { useEffect, useState, type DragEvent } from 'react';
import type { Game } from '../../stores/libraryStore';
import { cartToneClass } from '../../domain/cartTone';
import { displayTitle, formatRomSize, regionCode, regionTag } from '../../domain/romFormat';
import { coverSrc } from '../../domain/coverArt';
import { IconPlaySolid, IconStar, IconInfo } from '../common/icons';

interface GameCardProps {
  game: Game;
  onPlay: (game: Game) => void;
  onDetails?: (game: Game) => void;
  onToggleFavorite?: (game: Game) => void;
  /** Staggered entrance delay in ms, set by the grid. */
  revealDelay?: number;
  /** Absolute covers directory, needed to build a renderable image src. */
  coversDir?: string | null;
}

/**
 * The library's signature element: a game drawn as a Super Famicom cartridge.
 *
 * Cover art for SNES ROMs is the exception, not the rule, and the previous card
 * rendered every artless game as the same game-pad emoji on a grey 16:9 tile --
 * a shelf where nothing was distinguishable and nothing looked like software.
 *
 * So the artless case is the designed case. Each card is a cart face: the
 * chamfered top-right shoulder that keeps a real cart from going in backwards,
 * a label field tinted from one of eight tones hashed off the title (stable for
 * the life of the library, so the colour becomes part of how you recognise the
 * game), the ridge hatching moulded above the label recess, and the header's own
 * data set in the register face where a cart carries its part number.
 *
 * When cover art does exist it fills the frame and the tone survives as the left
 * spine edge, so the grid keeps both its rhythm and its colour-coding.
 */
export function GameCard({
  game,
  onPlay,
  onDetails,
  onToggleFavorite,
  revealDelay = 0,
  coversDir = null,
}: GameCardProps) {
  const tone = cartToneClass(game.title);
  const title = displayTitle(game.title);
  const art = coverSrc(game, coversDir);
  // Reset per src so a re-fetched cover gets another chance.
  const [artBroken, setArtBroken] = useState(false);
  useEffect(() => setArtBroken(false), [art]);

  // Dragging a card onto a collection in the sidebar files it there. The card is
  // the drag source rather than a separate list, because the thing you want to
  // file is the thing you are looking at.
  const handleDragStart = (e: DragEvent) => {
    e.dataTransfer.setData('gameId', game.id);
    e.dataTransfer.effectAllowed = 'copy';
  };

  return (
    <div
      className={`cart ${tone} animate-rise group`}
      style={{ animationDelay: `${revealDelay}ms` }}
      draggable
      onDragStart={handleDragStart}
    >
      {art && !artBroken && <span className="cart-spine" aria-hidden />}

      <div className={`cart-label${art && !artBroken ? ' cart-label--art' : ''}`}>
        {art && !artBroken ? (
          <img
            src={art}
            alt=""
            className="cart-art"
            loading="lazy"
            // Fall back to the cartridge label if the file behind `cover_file` is
            // gone -- a moved data directory, or a cache clear whose database
            // write failed. Without this the card rendered as a bare tinted
            // rectangle with no title at all, and `gamesNeedingCovers` (which
            // filters on `cover_file` being unset) reported "all covered" and
            // disabled the only button that would have repaired it.
            onError={() => setArtBroken(true)}
          />
        ) : (
          <h3 className="cart-title" title={game.title}>
            {title}
          </h3>
        )}

        {onToggleFavorite && (
          <button
            type="button"
            onClick={() => onToggleFavorite(game)}
            className={`cart-fav ${game.favorite ? 'cart-fav--on' : ''}`}
            aria-pressed={game.favorite}
            title={game.favorite ? 'Remove from favourites' : 'Add to favourites'}
            aria-label={
              game.favorite
                ? `Remove ${title} from favourites`
                : `Add ${title} to favourites`
            }
          >
            <IconStar size={14} filled={game.favorite} />
          </button>
        )}

        {/* Play is the card's primary action, so it owns the label area on
            hover; details stays a small secondary control in the foot. */}
        <div className="cart-play">
          <button
            type="button"
            onClick={() => onPlay(game)}
            className="cart-play-btn"
            title={`Play ${title}`}
            aria-label={`Play ${title}`}
          >
            <IconPlaySolid size={18} />
          </button>
        </div>
      </div>

      <div className="cart-foot">
        <span
          className="register truncate"
          title={`${regionTag(game.country)} · ${game.rom_type} · ${formatRomSize(game.file_size)}`}
        >
          {regionCode(game.country)} · {formatRomSize(game.file_size)}
        </span>
        {onDetails && (
          <button
            type="button"
            onClick={() => onDetails(game)}
            className="flex-none text-mute transition-colors hover:text-ink"
            title={`Details for ${title}`}
            aria-label={`Details for ${title}`}
          >
            <IconInfo size={14} />
          </button>
        )}
      </div>
    </div>
  );
}
