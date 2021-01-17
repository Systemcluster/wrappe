pub use zerocopy::AsBytes;

pub const WRAPPE_FORMAT: u8 = 200;
pub const NAME_SIZE: usize = 128;

#[repr(C, packed)]
#[derive(AsBytes)]
pub struct StarterInfo {
    pub signature:        [u8; 8],
    pub show_console:     u8,
    pub current_dir:      u8,
    pub verification:     u8,
    pub show_information: u8,
    pub uid:              [u8; 16],
    pub unpack_target:    u8,
    pub versioning:       u8,
    pub wrappe_format:    u8,
    pub unpack_directory: [u8; NAME_SIZE],
    pub command:          [u8; NAME_SIZE],
}

#[repr(C, packed)]
#[derive(AsBytes)]
pub struct PayloadHeader {
    pub kind:               u8,
    pub directory_sections: usize,
    pub file_sections:      usize,
    pub symlink_sections:   usize,
    pub section_hash:       u64,
    pub payload_size:       u64,
}
impl PayloadHeader {
    pub fn len(&self) -> usize {
        self.directory_sections + self.file_sections + self.symlink_sections
    }
}
#[repr(C, packed)]
#[derive(AsBytes)]
pub struct DirectorySection {
    pub name:   [u8; NAME_SIZE],
    pub parent: u32,
}
#[repr(C, packed)]
#[derive(AsBytes)]
pub struct FileSectionHeader {
    pub position:              u64,
    pub size:                  u64,
    pub name:                  [u8; NAME_SIZE],
    pub parent:                u32,
    pub file_hash:             u64,
    pub compressed_hash:       u64,
    pub time_accessed_seconds: u64,
    pub time_accessed_nanos:   u32,
    pub time_modified_seconds: u64,
    pub time_modified_nanos:   u32,
    pub mode:                  u32,
    pub readonly:              u8,
}
#[repr(C, packed)]
#[derive(AsBytes)]
pub struct SymlinkSection {
    pub name:                  [u8; NAME_SIZE],
    pub parent:                u32,
    pub kind:                  u8,
    pub target:                u32,
    pub time_accessed_seconds: u64,
    pub time_accessed_nanos:   u32,
    pub time_modified_seconds: u64,
    pub time_modified_nanos:   u32,
    pub mode:                  u32,
    pub readonly:              u8,
}
