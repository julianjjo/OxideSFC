/// Video RAM (VRAM) for the SNES PPU
/// 
/// VRAM is 64KB of memory used to store tile data and tilemaps.
/// It's word-organized (16-bit), meaning addresses are divided by 2
/// for 16-bit operations, but accessed as bytes here.
/// 
/// Address range: $0000-$FFFF (within PPU address space $2100-$21FF)
pub struct Vram {
    /// 64KB VRAM data array. Boxed so this lives on the heap instead of
    /// inline in `Vram`/`Ppu`/`SystemBus` -- constructing a chain of
    /// `Self { field: Type::new() }` literals returns each nested struct by
    /// value, and in an unoptimized build those copies aren't guaranteed to
    /// be elided, so a 64KB inline array here multiplied across a few
    /// nested `new()` calls was enough to overflow the default 1MB thread
    /// stack the first time `SystemBus::new()` ran (i.e. on ROM load).
    data: Box<[u8; 65536]>,
}

impl Vram {
    pub fn new() -> Self {
        Self {
            data: vec![0u8; 65536].into_boxed_slice().try_into().unwrap(),
        }
    }

    /// Reads a byte from VRAM at the given address
    /// 
    /// # Arguments
    /// * `addr` - 16-bit VRAM address ($0000-$FFFF)
    /// 
    /// # Returns
    /// The byte at the specified address
    #[inline]
    pub fn read(&self, addr: u16) -> u8 {
        self.data[addr as usize]
    }

    /// Reads a word (16-bit) from VRAM at the given address
    /// 
    /// VRAM is word-organized, so this reads two consecutive bytes.
    /// The address is automatically masked to ensure word alignment.
    /// 
    /// # Arguments
    /// * `addr` - 16-bit VRAM address (will be masked to even address)
    /// 
    /// # Returns
    /// The 16-bit word at the specified address (little-endian: low byte first)
    #[inline]
    pub fn read_word(&self, addr: u16) -> u16 {
        let addr = addr & 0xFFFE; // Mask to ensure even address
        let low = self.data[addr as usize] as u16;
        let high = self.data[(addr + 1) as usize] as u16;
        low | (high << 8)
    }

    /// Writes a byte to VRAM at the given address
    /// 
    /// # Arguments
    /// * `addr` - 16-bit VRAM address ($0000-$FFFF)
    /// * `value` - Byte value to write
    #[inline]
    pub fn write(&mut self, addr: u16, value: u8) {
        self.data[addr as usize] = value;
    }

    /// Writes a word (16-bit) to VRAM at the given address
    /// 
    /// VRAM is word-organized, so this writes two consecutive bytes.
    /// The address is automatically masked to ensure word alignment.
    /// 
    /// # Arguments
    /// * `addr` - 16-bit VRAM address (will be masked to even address)
    /// * `value` - 16-bit word value to write (little-endian: low byte first)
    #[inline]
    pub fn write_word(&mut self, addr: u16, value: u16) {
        let addr = addr & 0xFFFE; // Mask to ensure even address
        self.data[addr as usize] = (value & 0xFF) as u8;
        self.data[(addr + 1) as usize] = ((value >> 8) & 0xFF) as u8;
    }

    /// Gets a mutable reference to the VRAM data
    /// Useful for bulk operations like DMA transfers
    /// 
    /// # Returns
    /// Mutable slice of the entire VRAM
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data[..]
    }

    /// Gets a reference to the VRAM data
    /// Useful for bulk reads like DMA transfers
    /// 
    /// # Returns
    /// Immutable slice of the entire VRAM
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..]
    }

    /// Clears VRAM to all zeros
    pub fn clear(&mut self) {
        self.data.fill(0);
    }
}

impl Default for Vram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vram_basic_read_write() {
        let mut vram = Vram::new();
        
        // Write and read a byte
        vram.write(0x1234, 0xAB);
        assert_eq!(vram.read(0x1234), 0xAB);
    }

    #[test]
    fn vram_word_read_write() {
        let mut vram = Vram::new();
        
        // Write and read a word (16-bit)
        vram.write_word(0x1000, 0xABCD);
        assert_eq!(vram.read_word(0x1000), 0xABCD);
    }

    #[test]
    fn vram_word_alignment() {
        let mut vram = Vram::new();
        
        // Write at odd address, read from even address
        vram.write_word(0x1001, 0xABCD);
        // Should be masked to 0x1000
        assert_eq!(vram.read_word(0x1000), 0xABCD);
    }

    #[test]
    fn vram_boundary_addresses() {
        let mut vram = Vram::new();
        
        // First address
        vram.write(0x0000, 0x12);
        assert_eq!(vram.read(0x0000), 0x12);
        
        // Last address
        vram.write(0xFFFF, 0x34);
        assert_eq!(vram.read(0xFFFF), 0x34);
    }

    #[test]
    fn vram_clear() {
        let mut vram = Vram::new();
        
        // Write some data
        vram.write(0x5000, 0xFF);
        assert_eq!(vram.read(0x5000), 0xFF);
        
        // Clear and verify
        vram.clear();
        assert_eq!(vram.read(0x5000), 0x00);
    }

    #[test]
    fn vram_multiple_writes() {
        let mut vram = Vram::new();
        
        // Write multiple values
        for i in 0..256 {
            vram.write(i as u16, i as u8);
        }
        
        // Verify all values
        for i in 0..256 {
            assert_eq!(vram.read(i as u16), i as u8);
        }
    }

    #[test]
    fn vram_slice_access() {
        let mut vram = Vram::new();
        
        // Write via slice
        vram.as_mut_slice()[0x100..0x110].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        
        // Read via slice
        let slice = vram.as_slice();
        assert_eq!(slice[0x100], 1);
        assert_eq!(slice[0x10F], 16);
    }
}
