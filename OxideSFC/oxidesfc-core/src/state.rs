//! Minimal binary save-state primitives shared by every component's
//! `save_state`/`load_state` pair. Deliberately not serde: the core keeps
//! its "no dependencies beyond bitflags" constraint, and an explicit
//! little-endian byte format keeps the state layout stable and auditable.

use crate::error::EmulationError;

/// Magic + version prefix for whole-machine snapshots.
const SNAPSHOT_MAGIC: &[u8; 4] = b"OXSF";
const SNAPSHOT_VERSION: u16 = 2;

/// Serializes the whole machine (CPU + everything `SystemBus` owns) into
/// a versioned byte buffer. The ROM itself is not included -- a snapshot
/// can only be restored onto a machine with the same cartridge loaded.
pub fn save_snapshot(cpu: &crate::cpu::Cpu, bus: &crate::bus::SystemBus) -> Vec<u8> {
    let mut out = Vec::with_capacity(0x40000);
    out.extend_from_slice(SNAPSHOT_MAGIC);
    put_u16(&mut out, SNAPSHOT_VERSION);
    cpu.save_state(&mut out);
    bus.save_state(&mut out);
    out
}

/// Restores a snapshot produced by `save_snapshot`. Fails without
/// modifying anything if the magic/version don't match; the CPU/bus may
/// be partially modified if the buffer turns out to be truncated further
/// in (the caller should treat any error as "reload the ROM").
pub fn load_snapshot(
    cpu: &mut crate::cpu::Cpu,
    bus: &mut crate::bus::SystemBus,
    data: &[u8],
) -> Result<(), EmulationError> {
    let mut r = StateReader::new(data);
    if r.bytes(4)? != SNAPSHOT_MAGIC {
        return Err(EmulationError::InvalidSaveState("bad snapshot magic"));
    }
    if r.u16()? != SNAPSHOT_VERSION {
        return Err(EmulationError::InvalidSaveState("unsupported snapshot version"));
    }
    cpu.load_state(&mut r)?;
    bus.load_state(&mut r)?;
    Ok(())
}

pub(crate) fn put_u8(out: &mut Vec<u8>, v: u8) {
    out.push(v);
}

pub(crate) fn put_bool(out: &mut Vec<u8>, v: bool) {
    out.push(v as u8);
}

pub(crate) fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_bytes(out: &mut Vec<u8>, v: &[u8]) {
    out.extend_from_slice(v);
}

/// Sequential reader over a save-state byte buffer. Every accessor
/// returns `InvalidSaveState` instead of panicking when the buffer is
/// truncated, so a corrupt/foreign file can never crash the emulator.
pub(crate) struct StateReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> StateReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], EmulationError> {
        if self.pos + n > self.data.len() {
            return Err(EmulationError::InvalidSaveState("truncated save state"));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, EmulationError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn bool(&mut self) -> Result<bool, EmulationError> {
        Ok(self.u8()? != 0)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, EmulationError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, EmulationError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, EmulationError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, EmulationError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], EmulationError> {
        self.take(n)
    }
}
