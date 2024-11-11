pub use zerocopy::{FromBytes, FromZeroes};

pub const WRAPPE_FORMAT: u8 = 203;
pub const WRAPPE_SIGNATURE_1: [u8; 6] = [0x50, 0x45, 0x33, 0x44, 0x00, 0x00];
pub const WRAPPE_SIGNATURE_2: [u8; 4] = [0x41, 0x54, 0x41, 0x00];
pub const NAME_SIZE: usize = 128;
pub const ARGS_SIZE: usize = 512;

#[repr(C, packed)]
#[derive(FromBytes, FromZeroes)]
pub struct StarterInfo {
    pub signature:        [u8; 8],
    pub show_console:     u8,
    pub current_dir:      u8,
    pub verification:     u8,
    pub show_information: u8,
    pub uid:              [u8; 16],
    pub unpack_target:    u8,
    pub versioning:       u8,
    pub once:             u8,
    pub nocleanup:        u8,
    pub wrappe_format:    u8,
    pub unpack_directory: [u8; NAME_SIZE],
    pub command:          [u8; NAME_SIZE],
    pub arguments:        [u8; ARGS_SIZE],
}

#[repr(C, packed)]
#[derive(FromBytes, FromZeroes)]
pub struct PayloadHeader {
    pub directory_sections: u64,
    pub file_sections:      u64,
    pub symlink_sections:   u64,
    pub dictionary_size:    u64,
    pub section_hash:       u64,
    pub payload_size:       u64,
    pub sections_size:      u64,
    pub kind:               u8,
}
impl PayloadHeader {
    pub fn len(&self) -> u64 {
        self.directory_sections + self.file_sections + self.symlink_sections
    }
}
#[repr(C, packed)]
#[derive(FromBytes, FromZeroes)]
pub struct DirectorySection {
    pub name:   [u8; NAME_SIZE],
    pub parent: u32,
}
#[repr(C, packed)]
#[derive(FromBytes, FromZeroes)]
pub struct FileSectionHeader {
    pub position:              u64,
    pub size:                  u64,
    pub name:                  [u8; NAME_SIZE],
    pub file_hash:             u64,
    pub compressed_hash:       u64,
    pub time_accessed_seconds: u64,
    pub time_modified_seconds: u64,
    pub parent:                u32,
    pub mode:                  u32,
    pub time_accessed_nanos:   u32,
    pub time_modified_nanos:   u32,
    pub readonly:              u8,
}
#[repr(C, packed)]
#[derive(FromBytes, FromZeroes)]
pub struct SymlinkSection {
    pub name:                  [u8; NAME_SIZE],
    pub parent:                u32,
    pub target:                u32,
    pub time_accessed_seconds: u64,
    pub time_modified_seconds: u64,
    pub time_accessed_nanos:   u32,
    pub time_modified_nanos:   u32,
    pub mode:                  u32,
    pub kind:                  u8,
    pub readonly:              u8,
}
