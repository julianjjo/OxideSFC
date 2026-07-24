/// Object Attribute Memory (OAM) for the SNES PPU
/// 
/// OAM stores sprite/object data for the SNES.
/// It consists of:
/// - Primary OAM: 512 bytes (128 sprite entries × 4 bytes each)
/// - Secondary OAM: 32 bytes (for sprite selection during rendering)
/// Total: 544 bytes
/// 
/// Each sprite entry is 4 bytes:
/// - Byte 0: X position (low 8 bits; bit 8 lives in the secondary table)
/// - Byte 1: Y position (0-255, wrapping -- see `renderer::evaluate_sprites`)
/// - Byte 2: Tile number (low 8 bits; bit 8 lives in byte 3, bit 0)
/// - Byte 3: Attributes, real hardware layout `vhoopppN`:
///   - Bit 7: Vertical flip
///   - Bit 6: Horizontal flip
///   - Bits 5-4: Priority (0-3)
///   - Bits 3-1: Palette number (0-7)
///   - Bit 0: Name table select (bit 8 of the tile number)
///
/// Sprite size (small/large, per OBSEL's size pair) is NOT encoded in this
/// byte -- it lives in the secondary OAM table below (2 bits per sprite,
/// packed 4-sprites-per-byte: low bit = X position bit 8, high bit = size).
///
/// Address range: $00-$21F (544 bytes total)

pub struct Oam {
    /// 544 bytes of OAM data (512 primary + 32 secondary)
    data: [u8; 544],
}

impl Oam {
    pub fn new() -> Self {
        Self {
            data: [0u8; 544],
        }
    }

    /// Reads a byte from OAM at the given address
    /// 
    /// # Arguments
    /// * `addr` - 16-bit OAM address ($0000-$021F)
    /// 
    /// # Returns
    /// The byte at the specified address
    #[inline]
    pub fn read(&self, addr: u16) -> u8 {
        // 544 is NOT a power of two, so a bitmask (e.g. `& 0x21F`) is not
        // equivalent to wrapping modulo 544 -- it would alias most
        // addresses above 543 to the wrong slot instead of wrapping
        // around to $000. Real hardware wraps the OAM address at 544
        // bytes, so use an explicit modulo here.
        self.data[(addr as usize) % 544] // Wrap at 544 bytes
    }

    /// Writes a byte to OAM at the given address
    ///
    /// # Arguments
    /// * `addr` - 16-bit OAM address ($0000-$021F)
    /// * `value` - Byte value to write
    #[inline]
    pub fn write(&mut self, addr: u16, value: u8) {
        self.data[(addr as usize) % 544] = value; // Wrap at 544 bytes
    }

    /// Gets the Y position of a sprite
    /// 
    /// # Arguments
    /// * `sprite_idx` - Sprite index (0-127)
    /// 
    /// # Returns
    /// Y position (0-255)
    ///
    /// Byte 1 of the entry, not byte 0: these accessors used to have X and Y
    /// swapped -- the opposite of the real layout the renderer decodes and of
    /// this module's own doc comment. Nothing outside these tests called them,
    /// but any future caller would have silently transposed every sprite.
    pub fn get_y(&self, sprite_idx: u8) -> u8 {
        let idx = (sprite_idx as usize) & 0x7F; // Max 128 sprites
        self.data[idx * 4 + 1]
    }

    /// Sets the Y position of a sprite
    ///
    /// # Arguments
    /// * `sprite_idx` - Sprite index (0-127)
    /// * `y` - Y position (0-255)
    pub fn set_y(&mut self, sprite_idx: u8, y: u8) {
        let idx = (sprite_idx as usize) & 0x7F;
        self.data[idx * 4 + 1] = y;
    }

    /// Gets the X position of a sprite
    ///
    /// # Arguments
    /// * `sprite_idx` - Sprite index (0-127)
    ///
    /// # Returns
    /// X position, low 8 bits (bit 8 lives in the secondary table)
    pub fn get_x(&self, sprite_idx: u8) -> u8 {
        let idx = (sprite_idx as usize) & 0x7F;
        self.data[idx * 4]
    }

    /// Sets the X position of a sprite
    ///
    /// # Arguments
    /// * `sprite_idx` - Sprite index (0-127)
    /// * `x` - X position (0-255)
    pub fn set_x(&mut self, sprite_idx: u8, x: u8) {
        let idx = (sprite_idx as usize) & 0x7F;
        self.data[idx * 4] = x;
    }

    /// Gets the tile number of a sprite
    /// 
    /// # Arguments
    /// * `sprite_idx` - Sprite index (0-127)
    /// 
    /// # Returns
    /// Tile number
    pub fn get_tile(&self, sprite_idx: u8) -> u8 {
        let idx = (sprite_idx as usize) & 0x7F;
        self.data[idx * 4 + 2]
    }

    /// Sets the tile number of a sprite
    /// 
    /// # Arguments
    /// * `sprite_idx` - Sprite index (0-127)
    /// * `tile` - Tile number
    pub fn set_tile(&mut self, sprite_idx: u8, tile: u8) {
        let idx = (sprite_idx as usize) & 0x7F;
        self.data[idx * 4 + 2] = tile;
    }

    /// Gets the attributes of a sprite
    /// 
    /// # Arguments
    /// * `sprite_idx` - Sprite index (0-127)
    /// 
    /// # Returns
    /// Attributes byte
    pub fn get_attributes(&self, sprite_idx: u8) -> u8 {
        let idx = (sprite_idx as usize) & 0x7F;
        self.data[idx * 4 + 3]
    }

    /// Sets the attributes of a sprite
    /// 
    /// # Arguments
    /// * `sprite_idx` - Sprite index (0-127)
    /// * `attrs` - Attributes byte
    pub fn set_attributes(&mut self, sprite_idx: u8, attrs: u8) {
        let idx = (sprite_idx as usize) & 0x7F;
        self.data[idx * 4 + 3] = attrs;
    }

    /// Gets the palette number from attributes
    ///
    /// Real hardware attribute byte layout is `vhoopppN`; palette is bits
    /// 3-1. Matches the inline decode in `renderer.rs::draw_sprites`
    /// (`(attrs >> 1) & 0x07`).
    ///
    /// # Arguments
    /// * `attrs` - Attributes byte
    ///
    /// # Returns
    /// Palette number (0-7) - 3 bits
    #[inline]
    pub fn get_palette(attrs: u8) -> u8 {
        (attrs >> 1) & 0x07
    }

    /// Gets the priority from attributes
    ///
    /// Real hardware attribute byte layout is `vhoopppN`; priority is the
    /// 2-bit field at bits 5-4. Matches the inline decode in
    /// `renderer.rs::draw_sprites` (`(attrs >> 4) & 0x03`).
    ///
    /// # Arguments
    /// * `attrs` - Attributes byte
    ///
    /// # Returns
    /// Priority (0-3)
    #[inline]
    pub fn get_priority(attrs: u8) -> u8 {
        (attrs >> 4) & 0x03
    }

    /// Gets the horizontal flip from attributes
    ///
    /// Real hardware attribute byte layout is `vhoopppN`; horizontal flip
    /// is bit 6. Matches the inline decode in `renderer.rs::draw_sprites`
    /// (`attrs & 0x40`).
    ///
    /// # Arguments
    /// * `attrs` - Attributes byte
    ///
    /// # Returns
    /// Horizontal flip (0 = normal, 1 = flipped)
    #[inline]
    pub fn get_flip_h(attrs: u8) -> bool {
        (attrs & 0x40) != 0
    }

    /// Gets the vertical flip from attributes
    ///
    /// Real hardware attribute byte layout is `vhoopppN`; vertical flip is
    /// bit 7. Matches the inline decode in `renderer.rs::draw_sprites`
    /// (`attrs & 0x80`).
    ///
    /// # Arguments
    /// * `attrs` - Attributes byte
    ///
    /// # Returns
    /// Vertical flip (0 = normal, 1 = flipped)
    #[inline]
    pub fn get_flip_v(attrs: u8) -> bool {
        (attrs & 0x80) != 0
    }

    /// Gets the size bit for a sprite, per OBSEL's size pair.
    ///
    /// Unlike palette/priority/flip, size is NOT part of the byte-3
    /// attribute byte (that byte's 8 bits are fully consumed by
    /// `vhoopppN`). It lives in the secondary OAM table instead: 2 bits
    /// per sprite, packed 4-sprites-per-byte, where the low bit is the
    /// sprite's X-position bit 8 and the high bit is size. Matches the
    /// inline decode in `renderer.rs::draw_sprites`
    /// (`(high_table_byte >> (shift + 1)) & 0x01`).
    ///
    /// # Arguments
    /// * `high_table_byte` - the secondary OAM byte covering this sprite,
    ///   i.e. `oam.read(512 + sprite_idx as u16 / 4)`
    /// * `sprite_idx` - sprite index (0-127)
    ///
    /// # Returns
    /// Size bit (0 = small size, 1 = large size, per OBSEL's size pair)
    #[inline]
    pub fn get_size(high_table_byte: u8, sprite_idx: u8) -> u8 {
        let shift = (sprite_idx % 4) * 2;
        (high_table_byte >> (shift + 1)) & 0x01
    }

    /// Gets a mutable reference to the OAM data
    /// Useful for bulk operations like DMA transfers
    /// 
    /// # Returns
    /// Mutable slice of the entire OAM
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Gets a reference to the OAM data
    /// Useful for bulk reads like DMA transfers
    /// 
    /// # Returns
    /// Immutable slice of the entire OAM
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Gets a reference to primary OAM only
    /// 
    /// # Returns
    /// Slice of primary OAM (512 bytes)
    pub fn primary(&self) -> &[u8] {
        &self.data[0..512]
    }

    /// Gets a mutable reference to primary OAM only
    /// 
    /// # Returns
    /// Mutable slice of primary OAM (512 bytes)
    pub fn primary_mut(&mut self) -> &mut [u8] {
        &mut self.data[0..512]
    }

    /// Gets a reference to secondary OAM only
    /// 
    /// # Returns
    /// Slice of secondary OAM (32 bytes)
    pub fn secondary(&self) -> &[u8] {
        &self.data[512..544]
    }

    /// Gets a mutable reference to secondary OAM only
    /// 
    /// # Returns
    /// Mutable slice of secondary OAM (32 bytes)
    pub fn secondary_mut(&mut self) -> &mut [u8] {
        &mut self.data[512..544]
    }

    /// Clears OAM to all zeros
    pub fn clear(&mut self) {
        self.data = [0u8; 544];
    }

    /// Clears only primary OAM
    pub fn clear_primary(&mut self) {
        self.data[0..512].fill(0);
    }

    /// Clears only secondary OAM
    pub fn clear_secondary(&mut self) {
        self.data[512..544].fill(0);
    }

    /// Gets the number of sprite entries
    pub const fn num_sprites(&self) -> usize {
        128
    }

    /// Gets the size in bytes
    pub const fn size(&self) -> usize {
        544
    }
}

impl Default for Oam {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oam_basic_read_write() {
        let mut oam = Oam::new();
        
        // Write and read a byte
        oam.write(0x00, 0xAB);
        assert_eq!(oam.read(0x00), 0xAB);
    }

    #[test]
    fn oam_address_wrapping() {
        let mut oam = Oam::new();

        // Address should wrap at 544 bytes (544 is NOT a power of two, so
        // this must be a true modulo-544 wrap, not a bitmask).
        // 0x400 = 1024, and 1024 % 544 = 480 -- NOT 0. A `& 0x21F` bitmask
        // would incorrectly alias this to index 0; the correct wrap lands
        // on index 480.
        oam.write(0x21F, 0x12);
        oam.write(0x400, 0x34); // Wraps to 1024 % 544 = 480
        assert_eq!(oam.read(0x21F), 0x12);
        assert_eq!(oam.read(480), 0x34);

        // 544 itself (one past the last valid address) must wrap to 0,
        // exercising the exact boundary where a bitmask would instead
        // alias to 512 (544 & 0x21F == 512).
        oam.write(544, 0x56);
        assert_eq!(oam.read(0), 0x56);
    }

    #[test]
    fn oam_sprite_access() {
        let mut oam = Oam::new();
        
        // Set sprite 0 data
        oam.set_y(0, 50);
        oam.set_x(0, 100);
        oam.set_tile(0, 0x20);
        oam.set_attributes(0, 0x05); // Palette 5
        
        // Verify sprite 0
        assert_eq!(oam.get_y(0), 50);
        assert_eq!(oam.get_x(0), 100);
        assert_eq!(oam.get_tile(0), 0x20);
        assert_eq!(oam.get_attributes(0), 0x05);
    }

    #[test]
    fn oam_attribute_parsing() {
        // Test attribute extraction
        // Real SNES OAM byte-3 attribute format is `vhoopppN`:
        // Bit 7: Vertical flip
        // Bit 6: Horizontal flip
        // Bits 5-4: Priority (0-3)
        // Bits 3-1: Palette number (0-7)
        // Bit 0: Name table select (tile number bit 8)
        // (Size is NOT in this byte -- see `oam_size_from_secondary_table`.)

        // Test palette bits (bits 3-1)
        let attrs = 0x0E; // 0000 1110: palette=7
        assert_eq!(Oam::get_palette(attrs), 7);

        let attrs = 0x06; // 0000 0110: palette=3
        assert_eq!(Oam::get_palette(attrs), 3);

        let attrs = 0x00;
        assert_eq!(Oam::get_palette(attrs), 0);

        // Test priority (bits 5-4, 2-bit value)
        let attrs = 0x10; // 0001 0000: priority=1
        assert_eq!(Oam::get_priority(attrs), 1);

        let attrs = 0x30; // 0011 0000: priority=3
        assert_eq!(Oam::get_priority(attrs), 3);

        let attrs = 0x00;
        assert_eq!(Oam::get_priority(attrs), 0);

        // Test flip horizontal (bit 6)
        let attrs = 0x40; // 0100 0000: flip_h=1
        assert!(Oam::get_flip_h(attrs));

        let attrs = 0x00;
        assert!(!Oam::get_flip_h(attrs));

        // Test flip vertical (bit 7)
        let attrs = 0x80; // 1000 0000: flip_v=1
        assert!(Oam::get_flip_v(attrs));

        let attrs = 0x00;
        assert!(!Oam::get_flip_v(attrs));

        // Test combined: v=1, h=1, priority=3, palette=5, name-select=1
        let attrs = 0xFB; // 1111 1011
        assert_eq!(Oam::get_palette(attrs), 5);
        assert_eq!(Oam::get_priority(attrs), 3);
        assert!(Oam::get_flip_h(attrs));
        assert!(Oam::get_flip_v(attrs));
    }

    #[test]
    fn oam_size_from_secondary_table() {
        // Size lives in the secondary OAM table, not the byte-3 attribute
        // byte: 2 bits per sprite, packed 4-sprites-per-byte, low bit = X
        // position bit 8, high bit = size.
        assert_eq!(Oam::get_size(0x02, 0), 1); // sprite 0: shift 0, bit 1 set
        assert_eq!(Oam::get_size(0x00, 0), 0);

        assert_eq!(Oam::get_size(0x08, 1), 1); // sprite 1: shift 2, bit 3 set
        assert_eq!(Oam::get_size(0x00, 1), 0);

        assert_eq!(Oam::get_size(0x80, 3), 1); // sprite 3: shift 6, bit 7 set
        assert_eq!(Oam::get_size(0x00, 3), 0);
    }

    #[test]
    fn oam_boundary_addresses() {
        let mut oam = Oam::new();
        
        // First address
        oam.write(0x000, 0x12);
        assert_eq!(oam.read(0x000), 0x12);
        
        // Last address (0x21F = 543)
        oam.write(0x21F, 0x34);
        assert_eq!(oam.read(0x21F), 0x34);
    }

    #[test]
    fn oam_high_sprite_index_does_not_alias_sprite_zero() {
        // Regression test for a bitmask-vs-modulo bug: 544 is not a power
        // of two, so masking an OAM byte address with `& 0x21F` is only
        // equivalent to wrapping modulo 544 for addresses that already fit
        // in 0..544. Real hardware addressing (via OAMADD auto-increment,
        // see `bus.rs`'s `oam_write`/`oam_read`) can present byte addresses
        // computed as `word_addr * 2 + high_bit`, which is not bounded to
        // 0..544 unless explicitly wrapped there -- so a raw bitmask could
        // alias high sprite indices back onto low ones.
        //
        // Sprite 8's entry starts at raw OAM offset 8 * 4 = 32 (X byte, with
        // Y at 33 -- see the module doc comment). Confirm writing there does
        // not disturb sprite 0's entry at offset 0, and that both are
        // independently addressable all the way up through sprite 127
        // (offset 127 * 4 = 508).
        let mut oam = Oam::new();

        oam.write(0, 0xAA); // sprite 0's X byte
        oam.write(32, 0xBB); // sprite 8's X byte
        assert_eq!(oam.read(0), 0xAA);
        assert_eq!(oam.read(32), 0xBB);
        assert_ne!(oam.read(32), oam.read(0));

        // Also verify via the sprite-index accessors, which route through
        // `idx * 4` and therefore exercise the same underlying addressing.
        oam.set_y(0, 0x11);
        oam.set_y(8, 0x22);
        assert_eq!(oam.get_y(0), 0x11);
        assert_eq!(oam.get_y(8), 0x22);
        assert_ne!(oam.get_y(8), oam.get_y(0));

        // Sprite 127 (last valid sprite, offset 508) must also be
        // independently addressable and not alias sprite 0 or sprite 8.
        oam.set_y(127, 0x33);
        assert_eq!(oam.get_y(127), 0x33);
        assert_eq!(oam.get_y(0), 0x11);
        assert_eq!(oam.get_y(8), 0x22);

        // A raw byte address computed the way OAMADD auto-increment does
        // on real hardware (word_addr * 2 [+ high byte]) for a word
        // address past the halfway point must still land in-range and
        // wrap correctly at 544, not alias back onto sprite 0..7 via a
        // power-of-two bitmask.
        let word_addr: u16 = 16; // word address for sprite 8's X/Y pair
        let byte_addr = word_addr.wrapping_mul(2); // = 32, sprite 8's X byte
        assert_eq!(byte_addr, 32);
        oam.write(byte_addr, 0x44);
        assert_eq!(oam.read(32), 0x44);
        assert_eq!(oam.read(1), 0x11); // sprite 0's Y (set above) untouched
    }

    #[test]
    fn oam_clear() {
        let mut oam = Oam::new();
        
        // Write some data
        oam.write(0x50, 0xFF);
        assert_eq!(oam.read(0x50), 0xFF);
        
        // Clear and verify
        oam.clear();
        assert_eq!(oam.read(0x50), 0x00);
    }

    #[test]
    fn oam_primary_secondary() {
        let mut oam = Oam::new();
        
        // Write to primary OAM
        oam.primary_mut()[0] = 0xAB;
        
        // Write to secondary OAM
        oam.secondary_mut()[0] = 0xCD;
        
        // Verify separation
        assert_eq!(oam.primary()[0], 0xAB);
        assert_eq!(oam.secondary()[0], 0xCD);
    }

    #[test]
    fn oam_multiple_sprites() {
        let mut oam = Oam::new();
        
        // Write multiple sprites (use smaller values to avoid overflow)
        for i in 0..128 {
            oam.set_y(i, i as u8);
            oam.set_x(i, (i * 2) as u8);
            oam.set_tile(i, i as u8);  // Use i instead of i*3 to avoid overflow
            oam.set_attributes(i, (i & 0x3F) as u8);  // Mask to avoid invalid attribute bits
        }
        
        // Verify all sprites
        for i in 0..128 {
            assert_eq!(oam.get_y(i), i as u8);
            assert_eq!(oam.get_x(i), (i * 2) as u8);
            assert_eq!(oam.get_tile(i), i as u8);
            assert_eq!(oam.get_attributes(i), (i & 0x3F) as u8);
        }
    }

    #[test]
    fn oam_sprite_index_wrap() {
        let mut oam = Oam::new();
        
        // Sprite index should wrap at 128
        oam.set_y(0, 10);
        oam.set_y(128, 20); // Should wrap to 0
        
        assert_eq!(oam.get_y(0), 20);
    }

    #[test]
    fn oam_slice_access() {
        let mut oam = Oam::new();
        
        // Write via slice
        oam.as_mut_slice()[0..16].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        
        // Read via slice
        let slice = oam.as_slice();
        assert_eq!(slice[0], 1);
        assert_eq!(slice[15], 16);
    }

    #[test]
    fn oam_constants() {
        let oam = Oam::new();
        assert_eq!(oam.num_sprites(), 128);
        assert_eq!(oam.size(), 544);
    }
}
