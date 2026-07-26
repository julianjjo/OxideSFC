/**
 * Formatting for the values a cartridge header actually reports.
 *
 * These are shared by the library card, the list row and the details panel so a
 * game reads identically wherever it appears.
 */

/**
 * ROM size in megabits.
 *
 * Megabits, not megabytes: it is the unit the hardware and its documentation
 * use, it is what was printed on the chips, and it is how anyone discussing
 * these carts refers to them ("a 32 Mbit cart", never "a 4 MB cart"). Showing
 * "4.0 MB" would be arithmetically fine and idiomatically wrong for this
 * audience.
 */
export function formatRomSize(bytes: number): string {
  if (!bytes || bytes < 0) return '—';

  // Discount a copier header before converting. Dumps made with a copier carry
  // an extra 512 bytes in front of the ROM, which is detectable because real
  // cart sizes are whole multiples of 1 KiB: only a headered dump leaves a
  // remainder of exactly 512. Without this, Super Mario World -- a 4 Mbit cart --
  // reported "4.0 Mbit" instead of "4 Mbit", advertising a precision that
  // describes the container rather than the cartridge.
  const romBytes = bytes % 1024 === 512 ? bytes - 512 : bytes;

  const mbit = (romBytes * 8) / (1024 * 1024);
  if (mbit < 1) return `${Math.round((romBytes * 8) / 1024)} Kbit`;
  // Cart sizes are powers of two, so a whole number is the norm; the decimal is
  // kept for the odd non-standard or trimmed dump.
  return `${mbit % 1 === 0 ? mbit : mbit.toFixed(1)} Mbit`;
}

/** Battery-backed save size, or null when the cart has no SRAM. */
export function formatSramSize(bytes: number): string | null {
  if (!bytes || bytes <= 0) return null;
  const kbit = (bytes * 8) / 1024;
  if (kbit < 1024) return `${kbit % 1 === 0 ? kbit : kbit.toFixed(1)} Kbit`;
  return `${((bytes * 8) / (1024 * 1024)).toFixed(0)} Mbit`;
}

/**
 * Short region code, for the cartridge card's foot.
 *
 * The card has room for roughly a dozen monospace characters beside its info
 * button, and `regionTag`'s "NTSC-U" spends half of that before the size is even
 * printed -- long enough that "NTSC-U · 32 Mbit" truncated to "NTSC-U · 32 Mb…".
 * The signal-timing form is kept for the list and details views, which have the
 * width for it.
 */
export function regionCode(country: string): string {
  const normalized = (country || '').trim().toLowerCase();
  const codes: Record<string, string> = {
    usa: 'USA',
    canada: 'CAN',
    japan: 'JPN',
    korea: 'KOR',
    europe: 'EUR',
    australia: 'AUS',
    brazil: 'BRA',
    france: 'FRA',
    germany: 'GER',
    spain: 'SPA',
    italy: 'ITA',
    sweden: 'SWE',
    netherlands: 'NLD',
  };
  return codes[normalized] || (country ? country.slice(0, 3).toUpperCase() : '???');
}

/** Signal-timing region tag, for readouts with room for it. */
export function regionTag(country: string): string {
  const normalized = (country || '').trim().toLowerCase();
  const tags: Record<string, string> = {
    usa: 'NTSC-U',
    japan: 'NTSC-J',
    europe: 'PAL',
    australia: 'PAL',
    brazil: 'PAL-M',
    canada: 'NTSC-U',
    france: 'PAL',
    germany: 'PAL',
    spain: 'PAL',
    italy: 'PAL',
    korea: 'NTSC-J',
    sweden: 'PAL',
    netherlands: 'PAL',
  };
  return tags[normalized] || (country ? country.toUpperCase() : 'UNKNOWN');
}

/** Total playtime, from seconds. */
export function formatPlayTime(seconds: number | null): string {
  if (seconds === null || seconds <= 0) return 'Never played';
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m`;
  return `${seconds}s`;
}

/**
 * Last-played, as a relative phrase.
 *
 * Relative rather than absolute because the only question this answers in a
 * library is "how recently?" -- an exact timestamp is noise when scanning a
 * shelf. The details panel still shows the full date.
 */
export function formatLastPlayed(iso: string | null): string {
  if (!iso) return 'Never';
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return 'Never';

  const seconds = Math.max(0, (Date.now() - then) / 1000);
  if (seconds < 90) return 'Just now';
  const minutes = seconds / 60;
  if (minutes < 60) return `${Math.round(minutes)} min ago`;
  const hours = minutes / 60;
  if (hours < 24) return `${Math.round(hours)}h ago`;
  const days = hours / 24;
  if (days < 7) return `${Math.round(days)}d ago`;
  if (days < 31) return `${Math.round(days / 7)}w ago`;
  if (days < 365) return `${Math.round(days / 30)}mo ago`;
  return `${Math.round(days / 365)}y ago`;
}

/** Full timestamp, for the details panel. */
export function formatDateTime(iso: string | null): string {
  if (!iso) return 'Never';
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return 'Never';
  return date.toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/**
 * Strip the release-group furniture from a dump's filename-derived title so the
 * card shows the game's name.
 *
 * Region and revision markers are dropped from the *display* title only; the
 * real region still shows in the card's readout, sourced from the ROM header
 * rather than from the filename.
 */
export function displayTitle(title: string): string {
  return (
    title
      // "(USA)", "(Japan) (Rev 1)", "[!]", "[b1]" ...
      .replace(/\s*[([][^)\]]*[)\]]/g, '')
      .replace(/\s{2,}/g, ' ')
      .trim() || title
  );
}
