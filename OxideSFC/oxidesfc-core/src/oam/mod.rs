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
mod tests;
