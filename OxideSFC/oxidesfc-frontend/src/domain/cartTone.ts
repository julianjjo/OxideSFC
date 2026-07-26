/**
 * Cartridge label tones.
 *
 * Most ROMs in a real library have no cover art, so the library used to render
 * every one of them with the same placeholder -- a shelf where nothing could be
 * told apart at a glance. Instead each game gets one of eight tones derived from
 * the Super Famicom's four face-button hues (see `--cart-0..7` in tokens.css),
 * chosen by hashing its identity.
 *
 * Two properties matter and are the reason this is a hash rather than an index:
 *
 *  - Stable. The same game keeps its tone across scans, sorts, filters and
 *    restarts, so the colour becomes part of how you recognise the title. An
 *    index into the sorted list would repaint the whole shelf whenever the sort
 *    order changed.
 *  - Spread. Titles that share a prefix ("Super Mario World", "Super Metroid")
 *    must not collapse onto one tone, which is what makes a first-letter bucket
 *    unusable for a SNES library specifically -- an unusual number of its titles
 *    begin with "Super".
 */

export const CART_TONE_COUNT = 8;

/**
 * FNV-1a (32-bit). Chosen for avalanche behaviour on short ASCII strings: every
 * byte is mixed into the whole accumulator, so shared prefixes stop mattering
 * after the first differing character.
 */
function fnv1a(input: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    // `Math.imul` keeps the multiply in 32-bit space; a plain `hash * 16777619`
    // exceeds Number.MAX_SAFE_INTEGER and silently loses the low bits that
    // carry the entropy.
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/**
 * Tone index (0-7) for a game.
 *
 * The key drops parenthesised and bracketed groups before hashing, so the tone
 * survives the cosmetic differences between two dumps of the same game:
 * "Super Metroid (USA)" and "Super Metroid (U) [!]" land on the same tone.
 * Stripping only non-alphanumerics was not enough -- it kept `usa` and `u` in
 * the key, so those two hashed apart and the documented behaviour was false.
 */
export function cartToneIndex(title: string): number {
  const key = title
    .replace(/[([][^)\]]*[)\]]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]/g, '');
  return fnv1a(key || title) % CART_TONE_COUNT;
}

/** The CSS class that binds `--cart-tone` for this game. */
export function cartToneClass(title: string): string {
  return `cart-tone-${cartToneIndex(title)}`;
}
