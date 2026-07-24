/// Color Graphics RAM (CGRAM) for the SNES PPU
/// 
/// CGRAM stores the color palette data used for rendering.
/// It contains 256 color entries, each 2 bytes (15-bit BGR555 format).
/// Total size: 512 bytes (256 colors × 2 bytes).
/// 
/// Address range: $00-$FF (256 color entries, each entry is 2 bytes)
/// 
/// Color format (BGR555):
/// - Bits 14-10: Blue (5 bits, 0-31)
/// - Bits 9-5: Green (5 bits, 0-31)
/// - Bits 4-0: Red (5 bits, 0-31)
/// - Bit 15: Unused/ignored

#[derive(Clone)]
pub struct Cgram {
    /// 512 bytes of color data (256 colors × 2 bytes each)
    colors: [u8; 512],
}

impl Cgram {
    pub fn new() -> Self {
        Self {
            colors: [0u8; 512],
        }
    }

    /// Reads a byte from CGRAM at the given address
    /// 
    /// # Arguments
    /// * `addr` - CGRAM address (0-511, wraps at 512)
    /// 
    /// # Returns
    /// The byte at the specified address (0-511)
    #[inline]
    pub fn read(&self, addr: u16) -> u8 {
        self.colors[(addr as usize) & 0x1FF] // Mask to 512 bytes
    }

    /// Writes a byte to CGRAM at the given address
    /// 
    /// # Arguments
    /// * `addr` - CGRAM address (0-511, wraps at 512)
    /// * `value` - Byte value to write
    #[inline]
    pub fn write(&mut self, addr: u16, value: u8) {
        self.colors[(addr as usize) & 0x1FF] = value; // Mask to 512 bytes
    }

    /// Reads a complete color entry (16 bits) from CGRAM
    /// 
    /// # Arguments
    /// * `color_idx` - Color index (0-255)
    /// 
    /// # Returns
    /// The 16-bit color value (little-endian: low byte first)
    /// Lower 15 bits contain BGR555 color data
    pub fn read_color(&self, color_idx: u8) -> u16 {
        let addr = (color_idx as usize) & 0xFF;
        let low = self.colors[addr * 2] as u16;
        let high = self.colors[addr * 2 + 1] as u16;
        low | (high << 8)
    }

    /// Writes a complete color entry (16 bits) to CGRAM
    /// 
    /// # Arguments
    /// * `color_idx` - Color index (0-255)
    /// * `color` - 16-bit color value to write (BGR555 format)
    pub fn write_color(&mut self, color_idx: u8, color: u16) {
        let addr = (color_idx as usize) & 0xFF;
        self.colors[addr * 2] = (color & 0xFF) as u8;
        self.colors[addr * 2 + 1] = ((color >> 8) & 0xFF) as u8;
    }

    /// Gets a mutable reference to the CGRAM data
    /// Useful for bulk operations like DMA transfers
    /// 
    /// # Returns
    /// Mutable slice of the entire CGRAM
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.colors
    }

    /// Gets a reference to the CGRAM data
    /// Useful for bulk reads like DMA transfers
    /// 
    /// # Returns
    /// Immutable slice of the entire CGRAM
    pub fn as_slice(&self) -> &[u8] {
        &self.colors
    }

    /// Clears CGRAM to all zeros (all black colors)
    pub fn clear(&mut self) {
        self.colors = [0u8; 512];
    }

    /// Gets the number of color entries
    pub const fn num_colors(&self) -> usize {
        256
    }

    /// Gets the size in bytes
    pub const fn size(&self) -> usize {
        512
    }

    /// Extracts the red component (5 bits) from a color
    /// 
    /// # Arguments
    /// * `color` - 16-bit color value in BGR555 format
    /// 
    /// # Returns
    /// Red value (0-31)
    #[inline]
    pub fn extract_red(color: u16) -> u8 {
        (color & 0x1F) as u8
    }

    /// Extracts the green component (5 bits) from a color
    /// 
    /// # Arguments
    /// * `color` - 16-bit color value in BGR555 format
    /// 
    /// # Returns
    /// Green value (0-31)
    #[inline]
    pub fn extract_green(color: u16) -> u8 {
        ((color >> 5) & 0x1F) as u8
    }

    /// Extracts the blue component (5 bits) from a color
    /// 
    /// # Arguments
    /// * `color` - 16-bit color value in BGR555 format
    /// 
    /// # Returns
    /// Blue value (0-31)
    #[inline]
    pub fn extract_blue(color: u16) -> u8 {
        ((color >> 10) & 0x1F) as u8
    }

    /// Creates a BGR555 color from RGB components
    /// 
    /// BGR555 format:
    /// - Bits 0-4: Red (0-31)
    /// - Bits 5-9: Green (0-31)
    /// - Bits 10-14: Blue (0-31)
    /// 
    /// # Arguments
    /// * `r` - Red value (0-31)
    /// * `g` - Green value (0-31)
    /// * `b` - Blue value (0-31)
    /// 
    /// # Returns
    /// 16-bit color value in BGR555 format
    #[inline]
    pub fn make_color(r: u8, g: u8, b: u8) -> u16 {
        ((r as u16) & 0x1F) | (((g as u16) & 0x1F) << 5) | (((b as u16) & 0x1F) << 10)
    }
}

impl Default for Cgram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgram_basic_read_write() {
        let mut cgram = Cgram::new();
        
        // Write and read a byte
        cgram.write(0x00, 0xAB);
        assert_eq!(cgram.read(0x00), 0xAB);
    }

    #[test]
    fn cgram_color_read_write() {
        let mut cgram = Cgram::new();
        
        // Write and read a complete color
        cgram.write_color(0x00, 0x7FFF); // All bits set (white in BGR555)
        assert_eq!(cgram.read_color(0x00), 0x7FFF);
    }

    #[test]
    fn cgram_address_wrapping() {
        let mut cgram = Cgram::new();
        
        // Address should wrap at 512 bytes (256 colors * 2)
        // 0x200 = 512, and 512 & 0x1FF = 0, so it wraps to 0
        cgram.write(0xFF, 0x12);
        cgram.write(0x200, 0x34); // This should wrap to 0x00
        assert_eq!(cgram.read(0xFF), 0x12);
        assert_eq!(cgram.read(0x00), 0x34);
    }

    #[test]
    fn cgram_boundary_addresses() {
        let mut cgram = Cgram::new();
        
        // First byte
        cgram.write(0x00, 0x12);
        assert_eq!(cgram.read(0x00), 0x12);
        
        // Last byte (index 511 = 0x1FF)
        cgram.write(0x1FF, 0x56); // Last byte
        assert_eq!(cgram.read(0x1FF), 0x56);
    }

    #[test]
    fn cgram_clear() {
        let mut cgram = Cgram::new();
        
        // Write some data
        cgram.write(0x50, 0xFF);
        assert_eq!(cgram.read(0x50), 0xFF);
        
        // Clear and verify
        cgram.clear();
        assert_eq!(cgram.read(0x50), 0x00);
    }

    #[test]
    fn cgram_color_extraction() {
        // Test color extraction functions
        let color = Cgram::make_color(31, 31, 31); // White
        assert_eq!(Cgram::extract_red(color), 31);
        assert_eq!(Cgram::extract_green(color), 31);
        assert_eq!(Cgram::extract_blue(color), 31);

        let color = Cgram::make_color(0, 0, 0); // Black
        assert_eq!(Cgram::extract_red(color), 0);
        assert_eq!(Cgram::extract_green(color), 0);
        assert_eq!(Cgram::extract_blue(color), 0);

        let color = Cgram::make_color(0x1F, 0x00, 0x00); // Red (BGR555: 000 00000 11111)
        // In BGR555: Red is in bits 0-4
        assert_eq!(Cgram::extract_red(color), 0x1F);
        assert_eq!(Cgram::extract_green(color), 0x00);
        assert_eq!(Cgram::extract_blue(color), 0x00);
    }

    #[test]
    fn cgram_multiple_colors() {
        let mut cgram = Cgram::new();
        
        // Write multiple color entries
        for i in 0..16 {
            let color = Cgram::make_color(i * 2, i * 3, i * 4);
            cgram.write_color(i, color);
        }
        
        // Verify all colors
        for i in 0..16 {
            let color = Cgram::make_color(i * 2, i * 3, i * 4);
            assert_eq!(cgram.read_color(i), color);
        }
    }

    #[test]
    fn cgram_slice_access() {
        let mut cgram = Cgram::new();
        
        // Write via slice
        cgram.as_mut_slice()[0..16].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        
        // Read via slice
        let slice = cgram.as_slice();
        assert_eq!(slice[0], 1);
        assert_eq!(slice[15], 16);
    }

    #[test]
    fn cgram_constants() {
        let cgram = Cgram::new();
        assert_eq!(cgram.num_colors(), 256);
        assert_eq!(cgram.size(), 512);
    }
}
