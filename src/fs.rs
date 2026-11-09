//! SamenFS1: an intentionally small, persistent, raw-block filesystem.
//!
//! Sector 0 is the superblock, sectors 1..=16 hold 128 fixed directory
//! entries, and sectors 17 onwards hold contiguous file data. One root
//! directory only; no journal, partitions, or crash-consistent transactions.

pub const BLOCK_SIZE: usize = 512;
pub const MAX_FILES: usize = 128;
pub const MAX_NAME: usize = 31;
pub const DIRECTORY_SECTORS: u32 = 16;
pub const DATA_START: u32 = 1 + DIRECTORY_SECTORS;
const ENTRY_SIZE: usize = 64;
const ENTRIES_PER_SECTOR: usize = BLOCK_SIZE / ENTRY_SIZE;
const MAGIC: &[u8; 8] = b"SAMENFS1";
const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    Io,
    TooSmall,
    NotFormatted,
    Corrupt,
    InvalidName,
    NotFound,
    DirectoryFull,
    NoSpace,
    TooLarge,
}

pub trait BlockDevice {
    fn sector_count(&self) -> u32;
    fn read_sector(&mut self, lba: u32, output: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError>;
    fn write_sector(&mut self, lba: u32, input: &[u8; BLOCK_SIZE]) -> Result<(), FsError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub active: bool,
    pub name_bytes: [u8; MAX_NAME],
    pub first_sector: u32,
    pub size: u32,
    pub sectors: u32,
}

impl FileInfo {
    const EMPTY: Self = Self {
        active: false,
        name_bytes: [0; MAX_NAME],
        first_sector: 0,
        size: 0,
        sectors: 0,
    };

    pub fn name(&self) -> &str {
        let len = self.name_bytes.iter().position(|&c| c == 0).unwrap_or(MAX_NAME);
        core::str::from_utf8(&self.name_bytes[..len]).unwrap_or("<invalid>")
    }
}

pub struct Fs {
    total_sectors: u32,
    entries: [FileInfo; MAX_FILES],
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name != "."
        && name != ".."
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

impl Fs {
    /// Destructively initialize a dedicated raw disk. Does not erase file data.
    pub fn format<D: BlockDevice>(disk: &mut D) -> Result<(), FsError> {
        let sectors = disk.sector_count();
        if sectors <= DATA_START {
            return Err(FsError::TooSmall);
        }
        let zero = [0u8; BLOCK_SIZE];
        // Invalidate the old filesystem first; never leave a partially fresh
        // directory behind a still-valid original superblock.
        let mut invalid = zero;
        disk.write_sector(0, &invalid)?;
        for lba in 1..DATA_START {
            disk.write_sector(lba, &zero)?;
        }
        invalid[..8].copy_from_slice(MAGIC);
        invalid[8..12].copy_from_slice(&VERSION.to_le_bytes());
        invalid[12..16].copy_from_slice(&sectors.to_le_bytes());
        invalid[16..20].copy_from_slice(&DIRECTORY_SECTORS.to_le_bytes());
        invalid[20..24].copy_from_slice(&DATA_START.to_le_bytes());
        disk.write_sector(0, &invalid)
    }

    pub fn mount<D: BlockDevice>(disk: &mut D) -> Result<Self, FsError> {
        if disk.sector_count() <= DATA_START {
            return Err(FsError::TooSmall);
        }
        let mut sector = [0u8; BLOCK_SIZE];
        disk.read_sector(0, &mut sector)?;
        if &sector[..8] != MAGIC {
            return Err(FsError::NotFormatted);
        }
        if read_u32(&sector[8..12]) != VERSION
            || read_u32(&sector[16..20]) != DIRECTORY_SECTORS
            || read_u32(&sector[20..24]) != DATA_START
        {
            return Err(FsError::Corrupt);
        }
        let total = read_u32(&sector[12..16]);
        if total <= DATA_START || total > disk.sector_count() {
            return Err(FsError::Corrupt);
        }

        let mut entries = [FileInfo::EMPTY; MAX_FILES];
        for dir_sector in 0..DIRECTORY_SECTORS {
            disk.read_sector(1 + dir_sector, &mut sector)?;
            for slot in 0..ENTRIES_PER_SECTOR {
                let raw = &sector[slot * ENTRY_SIZE..(slot + 1) * ENTRY_SIZE];
                let idx = dir_sector as usize * ENTRIES_PER_SECTOR + slot;
                if raw[0] == 0 {
                    continue;
                }
                if raw[0] != 1 {
                    return Err(FsError::Corrupt);
                }
                let mut name_bytes = [0u8; MAX_NAME];
                name_bytes.copy_from_slice(&raw[1..32]);
                let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(MAX_NAME);
                let name = core::str::from_utf8(&name_bytes[..name_end])
                    .map_err(|_| FsError::Corrupt)?;
                if !valid_name(name) || name_bytes[name_end..].iter().any(|&b| b != 0) {
                    return Err(FsError::Corrupt);
                }
                let first = read_u32(&raw[32..36]);
                let size = read_u32(&raw[36..40]);
                let count = read_u32(&raw[40..44]);
                if count == 0 {
                    if first != 0 || size != 0 {
                        return Err(FsError::Corrupt);
                    }
                } else if first < DATA_START
                    || first.checked_add(count).is_none_or(|end| end > total)
                    || size == 0
                    || u64::from(size) > u64::from(count) * BLOCK_SIZE as u64
                    || u64::from(size) <= u64::from(count - 1) * BLOCK_SIZE as u64
                {
                    return Err(FsError::Corrupt);
                }
                entries[idx] = FileInfo {
                    active: true,
                    name_bytes,
                    first_sector: first,
                    size,
                    sectors: count,
                };
            }
        }
        // Reject overlapping allocation ranges and duplicate names.
        for i in 0..MAX_FILES {
            if !entries[i].active {
                continue;
            }
            for j in 0..i {
                if !entries[j].active {
                    continue;
                }
                if entries[i].name() == entries[j].name() {
                    return Err(FsError::Corrupt);
                }
                let a = entries[i];
                let b = entries[j];
                if a.sectors > 0 && b.sectors > 0
                    && a.first_sector < b.first_sector + b.sectors
                    && b.first_sector < a.first_sector + a.sectors
                {
                    return Err(FsError::Corrupt);
                }
            }
        }
        Ok(Self { total_sectors: total, entries })
    }

    pub fn files(&self) -> impl Iterator<Item = &FileInfo> {
        self.entries.iter().filter(|e| e.active)
    }

    pub fn total_sectors(&self) -> u32 {
        self.total_sectors
    }

    pub fn used_sectors(&self) -> u32 {
        self.files().map(|e| e.sectors).sum()
    }

    pub fn info(&self, name: &str) -> Result<&FileInfo, FsError> {
        self.files().find(|e| e.name() == name).ok_or(FsError::NotFound)
    }

    fn entry_index(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.active && e.name() == name)
    }

    fn free_span(&self, count: u32) -> Option<u32> {
        if count == 0 {
            return Some(0);
        }
        let mut candidate = DATA_START;
        while candidate.checked_add(count)? <= self.total_sectors {
            let end = candidate + count;
            let mut advance = candidate;
            for entry in self.files() {
                if entry.sectors == 0 {
                    continue;
                }
                let occupied_end = entry.first_sector + entry.sectors;
                if candidate < occupied_end && entry.first_sector < end {
                    advance = advance.max(occupied_end);
                }
            }
            if advance == candidate {
                return Some(candidate);
            }
            candidate = advance;
        }
        None
    }

    fn persist_entry<D: BlockDevice>(&self, disk: &mut D, index: usize) -> Result<(), FsError> {
        let lba = 1 + (index / ENTRIES_PER_SECTOR) as u32;
        let offset = (index % ENTRIES_PER_SECTOR) * ENTRY_SIZE;
        let mut sector = [0u8; BLOCK_SIZE];
        disk.read_sector(lba, &mut sector)?;
        let raw = &mut sector[offset..offset + ENTRY_SIZE];
        raw.fill(0);
        let entry = self.entries[index];
        if entry.active {
            raw[0] = 1;
            raw[1..32].copy_from_slice(&entry.name_bytes);
            raw[32..36].copy_from_slice(&entry.first_sector.to_le_bytes());
            raw[36..40].copy_from_slice(&entry.size.to_le_bytes());
            raw[40..44].copy_from_slice(&entry.sectors.to_le_bytes());
        }
        disk.write_sector(lba, &sector)
    }

    /// Replace or create a whole file, writing file data before directory metadata.
    pub fn write_file<D: BlockDevice>(
        &mut self,
        disk: &mut D,
        name: &str,
        data: &[u8],
    ) -> Result<(), FsError> {
        if !valid_name(name) {
            return Err(FsError::InvalidName);
        }
        if data.len() > u32::MAX as usize {
            return Err(FsError::TooLarge);
        }
        let blocks = ((data.len() + BLOCK_SIZE - 1) / BLOCK_SIZE) as u32;
        let index = self.entry_index(name).or_else(|| self.entries.iter().position(|e| !e.active))
            .ok_or(FsError::DirectoryFull)?;
        let old = self.entries[index];
        let start = if blocks == 0 {
            0
        } else if old.active && old.sectors >= blocks && old.sectors > 0 {
            old.first_sector
        } else {
            self.free_span(blocks).ok_or(FsError::NoSpace)?
        };

        for block in 0..blocks {
            let from = block as usize * BLOCK_SIZE;
            let to = (from + BLOCK_SIZE).min(data.len());
            let mut sector = [0u8; BLOCK_SIZE];
            sector[..to - from].copy_from_slice(&data[from..to]);
            disk.write_sector(start + block, &sector)?;
        }
        let mut name_bytes = [0u8; MAX_NAME];
        name_bytes[..name.len()].copy_from_slice(name.as_bytes());
        self.entries[index] = FileInfo {
            active: true,
            name_bytes,
            first_sector: start,
            size: data.len() as u32,
            sectors: blocks,
        };
        if let Err(err) = self.persist_entry(disk, index) {
            self.entries[index] = old;
            return Err(err);
        }
        Ok(())
    }

    /// Read file content in bounded 512-byte chunks; no allocation required.
    pub fn read_file<D: BlockDevice, F: FnMut(&[u8])>(
        &self,
        disk: &mut D,
        name: &str,
        mut receive: F,
    ) -> Result<(), FsError> {
        let entry = *self.info(name)?;
        let mut remaining = entry.size as usize;
        let mut sector = [0u8; BLOCK_SIZE];
        for block in 0..entry.sectors {
            disk.read_sector(entry.first_sector + block, &mut sector)?;
            let n = remaining.min(BLOCK_SIZE);
            receive(&sector[..n]);
            remaining -= n;
        }
        Ok(())
    }

    pub fn delete_file<D: BlockDevice>(
        &mut self,
        disk: &mut D,
        name: &str,
    ) -> Result<(), FsError> {
        let index = self.entry_index(name).ok_or(FsError::NotFound)?;
        let old = self.entries[index];
        self.entries[index] = FileInfo::EMPTY;
        if let Err(err) = self.persist_entry(disk, index) {
            self.entries[index] = old;
            return Err(err);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    struct MemoryDisk(Vec<[u8; BLOCK_SIZE]>);
    impl MemoryDisk {
        fn new(size: usize) -> Self { Self(vec![[0; BLOCK_SIZE]; size]) }
    }
    impl BlockDevice for MemoryDisk {
        fn sector_count(&self) -> u32 { self.0.len() as u32 }
        fn read_sector(&mut self, lba: u32, out: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
            *out = *self.0.get(lba as usize).ok_or(FsError::Io)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u32, input: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
            *self.0.get_mut(lba as usize).ok_or(FsError::Io)? = *input;
            Ok(())
        }
    }
    fn read(fs: &Fs, disk: &mut MemoryDisk, name: &str) -> Vec<u8> {
        let mut data = Vec::new();
        fs.read_file(disk, name, |chunk| data.extend_from_slice(chunk)).unwrap();
        data
    }

    #[test]
    fn format_create_remount_delete() {
        let mut disk = MemoryDisk::new(256);
        assert!(matches!(Fs::mount(&mut disk), Err(FsError::NotFormatted)));
        Fs::format(&mut disk).unwrap();
        let mut fs = Fs::mount(&mut disk).unwrap();
        fs.write_file(&mut disk, "hello.txt", b"hello Samen").unwrap();
        fs.write_file(&mut disk, "empty", b"").unwrap();
        assert_eq!(read(&fs, &mut disk, "hello.txt"), b"hello Samen");
        assert_eq!(read(&fs, &mut disk, "empty"), b"");
        drop(fs);
        let mut fs = Fs::mount(&mut disk).unwrap();
        assert_eq!(read(&fs, &mut disk, "hello.txt"), b"hello Samen");
        fs.delete_file(&mut disk, "hello.txt").unwrap();
        assert!(matches!(fs.info("hello.txt"), Err(FsError::NotFound)));
        fs.write_file(&mut disk, "new.txt", b"reused").unwrap();
        assert_eq!(fs.info("new.txt").unwrap().first_sector, DATA_START);
    }

    #[test]
    fn multi_sector_replace_and_reuse() {
        let mut disk = MemoryDisk::new(80);
        Fs::format(&mut disk).unwrap();
        let mut fs = Fs::mount(&mut disk).unwrap();
        let big = vec![0xAB; 1400];
        fs.write_file(&mut disk, "big", &big).unwrap();
        fs.write_file(&mut disk, "other", &[0xCD; 700]).unwrap();
        assert_eq!(fs.info("big").unwrap().sectors, 3);
        assert_eq!(fs.info("other").unwrap().first_sector, DATA_START + 3);
        fs.write_file(&mut disk, "big", b"small").unwrap();
        assert_eq!(read(&fs, &mut disk, "big"), b"small");
        fs.delete_file(&mut disk, "big").unwrap();
        fs.write_file(&mut disk, "again", &big).unwrap();
        assert_eq!(read(&fs, &mut disk, "again"), big);
        let fs2 = Fs::mount(&mut disk).unwrap();
        assert_eq!(fs2.files().count(), 2);
    }

    #[test]
    fn errors_and_directory_corruption() {
        let mut disk = MemoryDisk::new(24);
        Fs::format(&mut disk).unwrap();
        let mut fs = Fs::mount(&mut disk).unwrap();
        assert_eq!(fs.write_file(&mut disk, "../oops", b"x"), Err(FsError::InvalidName));
        assert_eq!(fs.write_file(&mut disk, "too-big", &[9; 8 * 512]), Err(FsError::NoSpace));
        assert_eq!(fs.delete_file(&mut disk, "missing"), Err(FsError::NotFound));
        disk.0[0][8] = 44;
        assert!(matches!(Fs::mount(&mut disk), Err(FsError::Corrupt)));
    }
}
