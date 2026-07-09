use crate::bus::{BusResult, MemoryBus};
use crate::error::EmulationError;

/// Work RAM - 128KB de RAM del SNES
/// Mapeada en $7E0000-$7FFFFF
pub struct Wram {
    data: Box<[u8; 0x20000]>, // 128KB = 131072 bytes
}

impl Wram {
    pub fn new() -> Self {
        Self {
            // Safe conversion: vec![0u8; 0x20000] always produces exactly 0x20000 bytes
            data: vec![0u8; 0x20000]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        }
    }
}

impl Wram {
    /// The full 128KB backing store, for save states.
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..]
    }

    /// Mutable access to the full 128KB backing store, for save states.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data[..]
    }
}

impl Default for Wram {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBus for Wram {
    fn read_u8(&mut self, addr: u32) -> BusResult<u8> {
        // WRAM está en $7E0000-$7FFFFF
        // Además, el banco 0 ($000000-$00FFFF) es un mirror de $7E0000-$7EFFFF
        // Esto permite que Direct Page (bank 0) funcione correctamente
        if (0x7E0000..0x800000).contains(&addr) {
            Ok(self.data[(addr - 0x7E0000) as usize])
        } else if addr < 0x2000 {
            // Mirror bank 0 to only the first 8KB of WRAM (Direct Page,
            // $0000-$1FFF). $2000-$7FFF is I/O registers and $8000-$FFFF
            // is ROM -- neither belongs to WRAM.
            Ok(self.data[addr as usize])
        } else {
            Err(EmulationError::InvalidAddress(addr))
        }
    }

    fn write_u8(&mut self, addr: u32, value: u8) -> BusResult<()> {
        if (0x7E0000..0x800000).contains(&addr) {
            self.data[(addr - 0x7E0000) as usize] = value;
            Ok(())
        } else if addr < 0x2000 {
            // Mirror bank 0 to only the first 8KB of WRAM (Direct Page,
            // $0000-$1FFF). $2000-$7FFF is I/O registers and $8000-$FFFF
            // is ROM -- neither belongs to WRAM.
            self.data[addr as usize] = value;
            Ok(())
        } else {
            Err(EmulationError::InvalidAddress(addr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wram_read_write() {
        let mut wram = Wram::new();
        wram.write_u8(0x7E1234, 0xAB).unwrap();
        assert_eq!(wram.read_u8(0x7E1234).unwrap(), 0xAB);
    }

    #[test]
    fn wram_invalid_address() {
        let mut wram = Wram::new();
        // Bank 0's Direct Page range ($0000-$1FFF) is valid (mirrors to WRAM)
        assert!(wram.read_u8(0x000000).is_ok());
        assert!(wram.read_u8(0x001FFF).is_ok());
        // $2000-$7FFF (I/O registers) and $8000-$FFFF (ROM) within bank 0
        // are NOT WRAM -- only the low 8KB mirrors.
        assert!(wram.read_u8(0x002000).is_err());
        assert!(wram.read_u8(0x005678).is_err());
        assert!(wram.read_u8(0x00FFFF).is_err());
        // Banks outside WRAM range are invalid
        assert!(wram.read_u8(0x010000).is_err()); // Bank 1
        assert!(wram.read_u8(0x800000).is_err());  // Bank 80+
    }

    #[test]
    fn wram_bank0_mirror() {
        let mut wram = Wram::new();
        // Write to bank 0, read from $7E mirror
        wram.write_u8(0x1234, 0xAB).unwrap();
        assert_eq!(wram.read_u8(0x7E1234).unwrap(), 0xAB);

        // Write to $7E, read from bank 0 (within the 8KB Direct Page mirror)
        wram.write_u8(0x7E1678, 0xCD).unwrap();
        assert_eq!(wram.read_u8(0x1678).unwrap(), 0xCD);
    }

    #[test]
    fn wram_boundary_addresses() {
        let mut wram = Wram::new();
        // First address
        wram.write_u8(0x7E0000, 0x12).unwrap();
        assert_eq!(wram.read_u8(0x7E0000).unwrap(), 0x12);

        // Last address
        wram.write_u8(0x7FFFFF, 0x34).unwrap();
        assert_eq!(wram.read_u8(0x7FFFFF).unwrap(), 0x34);
    }
}