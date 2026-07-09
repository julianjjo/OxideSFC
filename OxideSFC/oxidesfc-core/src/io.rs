//! I/O Registers module for PPU control.
//! 
//! Implements SNES I/O Register Map ($2100-$21FF) for controlling the PPU,
//! including display settings, background configuration, OAM, VRAM, CGRAM,
//! windows, and APU communication ports.

/// I/O Registers for PPU control.
/// 
/// Maps to SNES addresses $2100-$21FF.
/// Many addresses are mirrored; see individual register documentation.
#[derive(Debug, Clone)]
pub struct IoRegisters {
    // Display Control ($2100)
    /// INIDISP - Display control
    /// Bit 7: Force blank (1 = force blank, 0 = normal display)
    /// Bits 0-3: Brightness (0xF = full brightness, 0x0 = blank)
    pub inidisp: u8,
    
    // Object (Sprite) Settings ($2101)
    /// OBSEL - Object size and name table selection
    /// Bits 0-2: Object size
    /// Bits 3-5: Name select
    /// Bits 6-7: Name bound
    pub obsel: u8,
    
    // OAM Address ($2102-$2103)
    /// OAMADD - OAM address (low byte)
    pub oamaddl: u8,
    /// OAMADD - OAM address (high byte, bit 0 is the 9th bit of OAM address)
    pub oamaddh: u8,
    
    // OAM Data ($2104)
    /// OAMDATA - OAM data read/write
    pub oamdata: u8,
    
    // Background Mode ($2105)
    /// BGMODE - BG mode and settings
    /// Bits 0-3: BG mode (0-7)
    /// Bit 4: BG4 16x16 tiles
    /// Bit 5: BG3 priority
    pub bgmode: u8,
    
    // BG Screen Size ($2107-$210A)
    /// BG1SC - BG1 screen size
    pub bg1sc: u8,
    /// BG2SC - BG2 screen size
    pub bg2sc: u8,
    /// BG3SC - BG3 screen size
    pub bg3sc: u8,
    /// BG4SC - BG4 screen size
    pub bg4sc: u8,
    
    // BG Tile Address ($210B-$210C)
    /// BG12NBA - BG1 and BG2 tile address
    pub bg12nba: u8,
    /// BG34NBA - BG3 and BG4 tile address
    pub bg34nba: u8,
    
    // BG Scroll ($210D-$2114) - Fine scroll (16-bit)
    /// BG1HOFS - BG1 horizontal fine scroll (write to both bytes)
    pub bg1hoffs: u16,
    /// BG1VOFS - BG1 vertical fine scroll (write to both bytes)
    pub bg1voffs: u16,
    /// BG2HOFS - BG2 horizontal fine scroll
    pub bg2hoffs: u16,
    /// BG2VOFS - BG2 vertical fine scroll
    pub bg2voffs: u16,
    /// BG3HOFS - BG3 horizontal fine scroll
    pub bg3hoffs: u16,
    /// BG3VOFS - BG3 vertical fine scroll
    pub bg3voffs: u16,
    /// BG4HOFS - BG4 horizontal fine scroll
    pub bg4hoffs: u16,
    /// BG4VOFS - BG4 vertical fine scroll
    pub bg4voffs: u16,
    
    // VRAM ($2115-$2119)
    /// VMAIN - VRAM address increment mode
    /// Bits 0-1: Address increment mode (0 = increment after write, 1 = decrement, etc.)
    /// Bit 7: Address remapping (0 = word access, 1 = byte access)
    pub vmain: u8,
    /// VMADDL - VRAM address (low byte)
    pub vmaddl: u8,
    /// VMADDH - VRAM address (high byte, bits 0-6 for 15-bit address)
    pub vmaddh: u8,
    /// VMDATAL - VRAM data (low byte)
    pub vmdatal: u8,
    /// VMDATAH - VRAM data (high byte)
    pub vmdatah: u8,
    
    // CGRAM ($211A-$211B)
    /// CGADD - CGRAM address
    pub cgadd: u8,
    /// CGDATA - CGRAM data (auto-increment after write)
    pub cgdata: u8,
    
    // Window Settings ($2121-$2129)
    /// W12SEL - Window 1 and 2 settings
    pub w12sel: u8,
    /// W34SEL - Window 3 and 4 settings
    pub w34sel: u8,
    /// WH0 - Window 1 position (left/right edges)
    pub wh0: u8,
    /// WH1 - Window 2 position
    pub wh1: u8,
    /// WH2 - Window 3 position
    pub wh2: u8,
    /// WH3 - Window 4 position
    pub wh3: u8,
    /// WMMODE - Window mask mode
    pub wmmode: u8,
    /// TMW - Main screen window mask enable
    pub tmw: u8,
    /// TSW - Sub screen window mask enable
    pub tsw: u8,
    
    // Screen Enable ($212C-$212D)
    /// TM - Main screen enable (BG enable)
    pub tm: u8,
    /// TS - Sub screen enable
    pub ts: u8,
    
    // Video Mode ($2130)
    /// SETINI - Mode 7 settings, interlace, etc.
    /// Bit 0: Mode 7 extbg (external BG for Mode 7)
    /// Bit 1: Pseudo-hires
    /// Bit 2: Over scan (0 = normal, 1 = overscan)
    /// Bits 3-4: Interlace (0 = no interlace, 1 = interlace, 2-3 = pseudo)
    /// Bit 5: Mode 7 hflip
    pub setini: u8,
    
    // Window settings (mirrored from $2121-$2127)
    /// W12SEL - Window 1/2 settings (mirrored from $2121)
    pub w12sel_mirror: u8,
    /// W34SEL - Window 3/4 settings (mirrored from $2122)
    pub w34sel_mirror: u8,
    /// WH0 - Window 1 position (mirrored from $2123)
    pub wh0_mirror: u8,
    /// WH1 - Window 2 position (mirrored from $2124)
    pub wh1_mirror: u8,
    /// WH2 - Window 3 position (mirrored from $2125)
    pub wh2_mirror: u8,
    /// WH3 - Window 4 position (mirrored from $2126)
    pub wh3_mirror: u8,
    /// WMMODE - Window mask mode (mirrored from $2127)
    pub wmmode_mirror: u8,
    
    // APU I/O Ports ($2140-$2143)
    /// APU ports for communication with APU
    pub apu_ports: [u8; 4],
    
    // Status Registers (read-only, these reflect internal state)
    /// RDNMI - NMI status (read-only)
    /// Bit 7: NMI flag (1 = NMI occurred, write 1 to clear)
    /// Bits 0-6: Time of NMI (vblank counter)
    pub rdnmi: u8,
    /// TIMEUP - H/V timer status (read-only)
    /// Bit 7: H/V timer flag
    pub timeup: u8,
    /// HVBJOY - Joypad status (read-only)
    /// Bit 7: Joypad busy
    /// Bit 0: Auto-joypad read enabled
    pub hvbjoy: u8,
    
    // Internal state for reads
    /// Internal latch for read operations (some registers read different values on successive reads)
    pub read_latch: u8,
    /// OAM address (computed from oamaddl and oamaddh)
    oam_address: u16,
    /// VRAM address (computed from vmaddl and vmaddh)
    vram_address: u16,
    /// CGRAM address
    cgram_address: u8,
    /// Whether OAM address high byte was written (for 9-bit addressing)
    oam_high_written: bool,
    /// Fine scroll latch state (write to M publishes to the scroll value)
    fine_scroll_latch: u8,
}

impl IoRegisters {
    /// Create a new IoRegisters instance initialized to defaults.
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Get the current OAM address (9-bit).
    pub fn oam_address(&self) -> u16 {
        self.oam_address
    }
    
    /// Set the OAM address (9-bit).
    pub fn set_oam_address(&mut self, addr: u16) {
        self.oam_address = addr & 0x1FF;
    }
    
    /// Get the current VRAM address (15-bit).
    pub fn vram_address(&self) -> u16 {
        self.vram_address
    }
    
    /// Set the VRAM address (15-bit).
    pub fn set_vram_address(&mut self, addr: u16) {
        self.vram_address = addr & 0x7FFF;
    }
    
    /// Get the current CGRAM address.
    pub fn cgram_address(&self) -> u8 {
        self.cgram_address
    }
    
    /// Set the CGRAM address.
    pub fn set_cgram_address(&mut self, addr: u8) {
        self.cgram_address = addr & 0xFF;
    }
    
    /// Get VRAM address increment based on VMAIN.
    /// Returns the number of bytes to increment after each VRAM access.
    pub fn vram_increment(&self) -> u8 {
        match self.vmain & 0x03 {
            0x00 => 1,   // Increment by 1 after write
            0x01 => 32,  // Increment by 32 after write
            0x02 => 128, // Increment by 128 after write
            0x03 => 128, // Increment by 128 after write
            _ => 128,
        }
    }
    
    /// Read from an I/O register at the given address.
    /// 
    /// # Arguments
    /// * `addr` - The register address (lower 8 bits of $2100-$21FF)
    /// 
    /// # Returns
    /// The value read from the register
    pub fn read(&self, addr: u8) -> u8 {
        match addr {
            // Display control
            0x00 => self.inidisp,
            
            // Object settings
            0x01 => self.obsel,
            
            // OAM address (read returns current address)
            0x02 => (self.oam_address & 0xFF) as u8,
            0x03 => ((self.oam_address >> 8) & 0x01) as u8 | 0xFE,
            
            // OAM data - reads from OAM at current address
            // Note: The actual OAM read would be handled by the PPU
            0x04 => self.oamdata,
            
            // BG mode
            0x05 => self.bgmode,
            
            // BG screen size
            0x07 => self.bg1sc,
            0x08 => self.bg2sc,
            0x09 => self.bg3sc,
            0x0A => self.bg4sc,
            
            // BG tile address
            0x0B => self.bg12nba,
            0x0C => self.bg34nba,
            
            // Fine scroll - read returns current value
            0x0D => (self.bg1hoffs & 0xFF) as u8,
            0x0E => ((self.bg1hoffs >> 8) & 0xFF) as u8,
            0x0F => (self.bg1voffs & 0xFF) as u8,
            0x10 => ((self.bg1voffs >> 8) & 0xFF) as u8,
            0x11 => (self.bg2hoffs & 0xFF) as u8,
            0x12 => ((self.bg2hoffs >> 8) & 0xFF) as u8,
            0x13 => (self.bg2voffs & 0xFF) as u8,
            0x14 => ((self.bg2voffs >> 8) & 0xFF) as u8,
            
            // VRAM address
            0x15 => self.vmain,
            0x16 => (self.vram_address & 0xFF) as u8,
            0x17 => ((self.vram_address >> 8) & 0x7F) as u8,
            
            // VRAM data (returns current VRAM data at address)
            // Note: The actual VRAM read would be handled by the PPU
            0x18 => self.vmdatal,
            0x19 => self.vmdatah,
            
            // CGRAM address
            0x1A => self.cgadd,
            // CGRAM data (read returns current CGRAM data at address)
            0x1B => self.cgdata,
            
            // Window settings
            0x21 => self.w12sel,
            0x22 => self.w34sel,
            0x23 => self.wh0,
            0x24 => self.wh1,
            0x25 => self.wh2,
            0x26 => self.wh3,
            0x27 => self.wmmode,
            0x28 => self.tmw,
            0x29 => self.tsw,
            
            // Screen enable
            0x2C => self.tm,
            0x2D => self.ts,
            
            // Video mode
            0x30 => self.setini,
            
            // Status registers (read-only)
            // $213E = 0x3E: RDNMI
            // $213F = 0x3F: TIMEUP
            0x3E => self.rdnmi,
            0x3F => self.timeup,
            
            // Window settings (mirrored at $2131-$2137) - mirrors read from primary registers
            0x31 => self.w12sel,
            0x32 => self.w34sel,
            0x33 => self.wh0,
            0x34 => self.wh1,
            0x35 => self.wh2,
            0x36 => self.wh3,
            0x37 => self.wmmode,
            
            // APU ports ($2140-$2143 = 0x40-0x43)
            0x40..=0x43 => self.apu_ports[(addr - 0x40) as usize],
            // Mirrored at $2144-$2147 = 0x44-0x47
            0x44..=0x47 => self.apu_ports[(addr - 0x44) as usize],
            
            // Unimplemented/undefined registers return 0
            _ => 0x00,
        }
    }
    
    /// Write to an I/O register at the given address.
    /// 
    /// # Arguments
    /// * `addr` - The register address (lower 8 bits of $2100-$21FF)
    /// * `value` - The value to write
    pub fn write(&mut self, addr: u8, value: u8) {
        match addr {
            // Display control
            0x00 => self.inidisp = value,
            
            // Object settings
            0x01 => self.obsel = value,
            
            // OAM address
            0x02 => {
                self.oamaddl = value;
                self.oam_address = (self.oam_address & 0x100) | (value as u16);
            }
            0x03 => {
                self.oamaddh = value;
                self.oam_high_written = true;
                self.oam_address = (((value & 0x01) as u16) << 8) | (self.oam_address & 0xFF);
            }
            
            // OAM data
            0x04 => {
                self.oamdata = value;
                // Auto-increment OAM address after write
                self.oam_address = (self.oam_address + 1) & 0x1FF;
            }
            
            // BG mode
            0x05 => self.bgmode = value,
            
            // BG screen size
            0x07 => self.bg1sc = value,
            0x08 => self.bg2sc = value,
            0x09 => self.bg3sc = value,
            0x0A => self.bg4sc = value,
            
            // BG tile address
            0x0B => self.bg12nba = value,
            0x0C => self.bg34nba = value,
            
            // Fine scroll - these require two writes: first low byte, then high byte
            // The scroll values are latched and published on the second write
            0x0D => {
                // BG1HOFS low - latch
                self.fine_scroll_latch = value;
                self.bg1hoffs = (self.bg1hoffs & 0xFF00) | (value as u16);
            }
            0x0E => {
                // BG1HOFS high - publish
                self.bg1hoffs = ((value as u16) << 8) | (self.fine_scroll_latch as u16);
            }
            0x0F => {
                // BG1VOFS low - latch
                self.fine_scroll_latch = value;
                self.bg1voffs = (self.bg1voffs & 0xFF00) | (value as u16);
            }
            0x10 => {
                // BG1VOFS high - publish
                self.bg1voffs = ((value as u16) << 8) | (self.fine_scroll_latch as u16);
            }
            0x11 => {
                // BG2HOFS low - latch
                self.fine_scroll_latch = value;
                self.bg2hoffs = (self.bg2hoffs & 0xFF00) | (value as u16);
            }
            0x12 => {
                // BG2HOFS high - publish
                self.bg2hoffs = ((value as u16) << 8) | (self.fine_scroll_latch as u16);
            }
            0x13 => {
                // BG2VOFS low - latch
                self.fine_scroll_latch = value;
                self.bg2voffs = (self.bg2voffs & 0xFF00) | (value as u16);
            }
            0x14 => {
                // BG2VOFS high - publish
                self.bg2voffs = ((value as u16) << 8) | (self.fine_scroll_latch as u16);
            }
            
            // VMAIN - VRAM address increment mode ($2115)
            0x15 => self.vmain = value,
            
            // VMADDL - VRAM address low ($2116)
            0x16 => {
                self.vmaddl = value;
                self.vram_address = (self.vram_address & 0xFF00) | (value as u16);
            }
            // VMADDH - VRAM address high ($2117)
            0x17 => {
                self.vmaddh = value;
                self.vram_address = ((value as u16) & 0x7F) << 8 | (self.vram_address & 0xFF);
            }
            
            // VMDATAL - VRAM data low ($2118)
            0x18 => {
                self.vmdatal = value;
                // Auto-increment VRAM address
                self.vram_address = (self.vram_address + self.vram_increment() as u16) & 0x7FFF;
            }
            // VMDATAH - VRAM data high ($2119)
            0x19 => {
                self.vmdatah = value;
                // Auto-increment VRAM address
                self.vram_address = (self.vram_address + self.vram_increment() as u16) & 0x7FFF;
            }
            
            // CGADD - CGRAM address ($211A)
            0x1A => {
                self.cgadd = value;
                self.cgram_address = value;
            }
            // CGDATA - CGRAM data ($211B)
            0x1B => {
                self.cgdata = value;
                // Auto-increment CGRAM address
                self.cgram_address = self.cgram_address.wrapping_add(1);
            }
            
            // W12SEL - Window 1/2 settings ($2121)
            0x21 => self.w12sel = value,
            // W34SEL - Window 3/4 settings ($2122)
            0x22 => self.w34sel = value,
            // WH0 - Window 1 position ($2123)
            0x23 => self.wh0 = value,
            // WH1 - Window 2 position ($2124)
            0x24 => self.wh1 = value,
            // WH2 - Window 3 position ($2125)
            0x25 => self.wh2 = value,
            // WH3 - Window 4 position ($2126)
            0x26 => self.wh3 = value,
            // WMMODE - Window mask mode ($2127)
            0x27 => self.wmmode = value,
            // TMW - Main screen window ($2128)
            0x28 => self.tmw = value,
            // TSW - Sub screen window ($2129)
            0x29 => self.tsw = value,
            
            // TM - Main screen enable ($212C)
            0x2C => self.tm = value,
            // TS - Sub screen enable ($212D)
            0x2D => self.ts = value,
            
            // SETINI - Mode 7, interlace ($2130)
            0x30 => self.setini = value,
            
            // W12SEL mirror ($2131)
            0x31 => {
                self.w12sel_mirror = value;
                self.w12sel = value; // Also update primary
            }
            // W34SEL mirror ($2132)
            0x32 => {
                self.w34sel_mirror = value;
                self.w34sel = value;
            }
            // WH0 mirror ($2133)
            0x33 => {
                self.wh0_mirror = value;
                self.wh0 = value;
            }
            // WH1 mirror ($2134)
            0x34 => {
                self.wh1_mirror = value;
                self.wh1 = value;
            }
            // WH2 mirror ($2135)
            0x35 => {
                self.wh2_mirror = value;
                self.wh2 = value;
            }
            // WH3 mirror ($2136)
            0x36 => {
                self.wh3_mirror = value;
                self.wh3 = value;
            }
            // WMMODE mirror ($2137)
            0x37 => {
                self.wmmode_mirror = value;
                self.wmmode = value;
            }
            
            // APU ports
            0x40..=0x43 => self.apu_ports[(addr - 0x40) as usize] = value,
            0x44..=0x47 => self.apu_ports[(addr - 0x44) as usize] = value, // Mirrored
            
            // Ignore writes to read-only registers
            // $40, $41, $42 (RDNMI, TIMEUP, HVBJOY) are read-only
            
            // Undefined registers - ignore
            _ => {}
        }
    }
    
    /// Check if display is forced blank.
    pub fn is_forced_blank(&self) -> bool {
        (self.inidisp & 0x80) != 0
    }
    
    /// Get the current brightness level (0-15).
    pub fn brightness(&self) -> u8 {
        self.inidisp & 0x0F
    }
    
    /// Get the BG mode (0-7).
    pub fn bg_mode(&self) -> u8 {
        self.bgmode & 0x07
    }
    
    /// Check if Mode 7 extbg is enabled.
    pub fn mode7_extbg(&self) -> bool {
        (self.setini & 0x01) != 0
    }
    
    /// Check if interlace is enabled.
    pub fn is_interlaced(&self) -> bool {
        (self.setini & 0x04) != 0
    }
    
    /// Check if overscan is enabled.
    pub fn is_overscan(&self) -> bool {
        (self.setini & 0x08) != 0
    }
    
    /// Reset the I/O registers to default values.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Default for IoRegisters {
    fn default() -> Self {
        Self {
            inidisp: 0x00,
            obsel: 0x00,
            oamaddl: 0x00,
            oamaddh: 0x00,
            oamdata: 0x00,
            bgmode: 0x00,
            bg1sc: 0x00,
            bg2sc: 0x00,
            bg3sc: 0x00,
            bg4sc: 0x00,
            bg12nba: 0x00,
            bg34nba: 0x00,
            bg1hoffs: 0x0000,
            bg1voffs: 0x0000,
            bg2hoffs: 0x0000,
            bg2voffs: 0x0000,
            bg3hoffs: 0x0000,
            bg3voffs: 0x0000,
            bg4hoffs: 0x0000,
            bg4voffs: 0x0000,
            vmain: 0x00,
            vmaddl: 0x00,
            vmaddh: 0x00,
            vmdatal: 0x00,
            vmdatah: 0x00,
            cgadd: 0x00,
            cgdata: 0x00,
            w12sel: 0x00,
            w34sel: 0x00,
            wh0: 0x00,
            wh1: 0x00,
            wh2: 0x00,
            wh3: 0x00,
            wmmode: 0x00,
            tmw: 0x00,
            tsw: 0x00,
            tm: 0x00,
            ts: 0x00,
            setini: 0x00,
            w12sel_mirror: 0x00,
            w34sel_mirror: 0x00,
            wh0_mirror: 0x00,
            wh1_mirror: 0x00,
            wh2_mirror: 0x00,
            wh3_mirror: 0x00,
            wmmode_mirror: 0x00,
            apu_ports: [0x00; 4],
            rdnmi: 0x00,
            timeup: 0x00,
            hvbjoy: 0x00,
            read_latch: 0x00,
            oam_address: 0x0000,
            vram_address: 0x0000,
            cgram_address: 0x00,
            oam_high_written: false,
            fine_scroll_latch: 0x00,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_registers_default() {
        let regs = IoRegisters::new();
        
        // All registers should be initialized to 0
        assert_eq!(regs.inidisp, 0x00);
        assert_eq!(regs.obsel, 0x00);
        assert_eq!(regs.bgmode, 0x00);
        assert_eq!(regs.oam_address(), 0);
        assert_eq!(regs.vram_address(), 0);
    }

    #[test]
    fn test_write_inidisp() {
        let mut regs = IoRegisters::new();
        
        // Write display control with force blank and max brightness
        regs.write(0x00, 0x8F); // 1000 1111
        
        assert_eq!(regs.inidisp, 0x8F);
        assert!(regs.is_forced_blank());
        assert_eq!(regs.brightness(), 0x0F);
    }

    #[test]
    fn test_write_bgmode() {
        let mut regs = IoRegisters::new();
        
        // Write BG mode 1
        regs.write(0x05, 0x01);
        
        assert_eq!(regs.bgmode, 0x01);
        assert_eq!(regs.bg_mode(), 1);
    }

    #[test]
    fn test_oam_address() {
        let mut regs = IoRegisters::new();
        
        // Write OAM address low byte
        regs.write(0x02, 0x50);
        assert_eq!(regs.oam_address(), 0x50);
        
        // Write OAM address high byte (with bit 0 = 1 for 9-bit addressing)
        regs.write(0x03, 0x01);
        assert_eq!(regs.oam_address(), 0x150);
    }

    #[test]
    fn test_vram_address() {
        let mut regs = IoRegisters::new();
        
        // Write VRAM address low byte
        regs.write(0x16, 0x00);
        regs.write(0x17, 0x00);
        assert_eq!(regs.vram_address(), 0x0000);
        
        // Write VRAM address
        regs.write(0x16, 0x20);
        regs.write(0x17, 0x03); // 0x0320 = 800
        assert_eq!(regs.vram_address(), 0x0320);
    }

    #[test]
    fn test_cgram_address() {
        let mut regs = IoRegisters::new();
        
        // Write CGRAM address
        regs.write(0x1A, 0x10);
        
        assert_eq!(regs.cgadd, 0x10);
        assert_eq!(regs.cgram_address(), 0x10);
        
        // Write CGRAM data should auto-increment
        regs.write(0x1B, 0xFF);
        assert_eq!(regs.cgram_address(), 0x11);
    }

    #[test]
    fn test_fine_scroll() {
        let mut regs = IoRegisters::new();
        
        // Write BG1 scroll (must write low byte first, then high byte)
        regs.write(0x0D, 0x50); // Low byte
        regs.write(0x0E, 0x01); // High byte
        assert_eq!(regs.bg1hoffs, 0x0150);
        
        // Write vertical scroll
        regs.write(0x0F, 0xA0); // Low byte
        regs.write(0x10, 0x02); // High byte
        assert_eq!(regs.bg1voffs, 0x02A0);
    }

    #[test]
    fn test_screen_enable() {
        let mut regs = IoRegisters::new();
        
        // Write TM (main screen enable)
        regs.write(0x2C, 0x1F); // Enable BG1-BG4
        assert_eq!(regs.tm, 0x1F);
        
        // Write TS (sub screen enable)
        regs.write(0x2D, 0x0F);
        assert_eq!(regs.ts, 0x0F);
    }

    #[test]
    fn test_apu_ports() {
        let mut regs = IoRegisters::new();
        
        // Write to APU ports
        regs.write(0x40, 0x12);
        regs.write(0x41, 0x34);
        regs.write(0x42, 0x56);
        regs.write(0x43, 0x78);
        
        assert_eq!(regs.apu_ports[0], 0x12);
        assert_eq!(regs.apu_ports[1], 0x34);
        assert_eq!(regs.apu_ports[2], 0x56);
        assert_eq!(regs.apu_ports[3], 0x78);
        
        // Read back
        assert_eq!(regs.read(0x40), 0x12);
        assert_eq!(regs.read(0x41), 0x34);
        assert_eq!(regs.read(0x42), 0x56);
        assert_eq!(regs.read(0x43), 0x78);
    }

    #[test]
    fn test_window_settings() {
        let mut regs = IoRegisters::new();
        
        // Write window settings
        regs.write(0x21, 0xAA); // W12SEL
        regs.write(0x22, 0x55); // W34SEL
        regs.write(0x23, 0x10); // WH0
        regs.write(0x24, 0x20); // WH1
        regs.write(0x25, 0x30); // WH2
        regs.write(0x26, 0x40); // WH3
        regs.write(0x27, 0x01); // WMMODE
        
        assert_eq!(regs.w12sel, 0xAA);
        assert_eq!(regs.w34sel, 0x55);
        assert_eq!(regs.wh0, 0x10);
        assert_eq!(regs.wh1, 0x20);
        assert_eq!(regs.wh2, 0x30);
        assert_eq!(regs.wh3, 0x40);
        assert_eq!(regs.wmmode, 0x01);
    }

    #[test]
    fn test_mirrored_window_settings() {
        let mut regs = IoRegisters::new();
        
        // Write to mirrored addresses
        regs.write(0x31, 0xBB); // W12SEL mirror
        assert_eq!(regs.w12sel_mirror, 0xBB);
        assert_eq!(regs.w12sel, 0xBB); // Should also update primary
        
        regs.write(0x32, 0xCC); // W34SEL mirror
        assert_eq!(regs.w34sel_mirror, 0xCC);
        assert_eq!(regs.w34sel, 0xCC);
    }

    #[test]
    fn test_setini() {
        let mut regs = IoRegisters::new();
        
        // Write SETINI with various settings
        // Bit 0: Mode 7 extbg
        // Bit 2: Overscan
        // Bit 3-4: Interlace
        regs.write(0x30, 0x0D); // 0000 1101
        
        assert!(regs.mode7_extbg());
        assert!(regs.is_overscan());
        assert!(regs.is_interlaced());
    }

    #[test]
    fn test_read_write_mirror() {
        let mut regs = IoRegisters::new();
        
        // Write to primary address
        regs.write(0x21, 0xFF);
        
        // Read from mirrored address should return same value
        assert_eq!(regs.read(0x31), 0xFF);
    }

    #[test]
    fn test_reset() {
        let mut regs = IoRegisters::new();
        
        // Write some values
        regs.write(0x00, 0x8F);
        regs.write(0x05, 0x01);
        regs.write(0x40, 0xAB);
        
        // Reset
        regs.reset();
        
        // All values should be back to defaults
        assert_eq!(regs.inidisp, 0x00);
        assert_eq!(regs.bgmode, 0x00);
        assert_eq!(regs.apu_ports[0], 0x00);
    }
}

#[cfg(test)]
mod vram_increment_tests {
    use super::*;

    #[test]
    fn test_vram_increment_mode_0() {
        let mut regs = IoRegisters::new();
        regs.vmain = 0x00;
        assert_eq!(regs.vram_increment(), 1);
    }

    #[test]
    fn test_vram_increment_mode_1() {
        let mut regs = IoRegisters::new();
        regs.vmain = 0x01;
        assert_eq!(regs.vram_increment(), 32);
    }

    #[test]
    fn test_vram_increment_mode_2() {
        let mut regs = IoRegisters::new();
        regs.vmain = 0x02;
        assert_eq!(regs.vram_increment(), 128);
    }

    #[test]
    fn test_vram_increment_mode_3() {
        let mut regs = IoRegisters::new();
        regs.vmain = 0x03;
        assert_eq!(regs.vram_increment(), 128);
    }
}
