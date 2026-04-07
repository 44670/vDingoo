use crate::mem::Memory;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

pub struct GuestFs {
    files: HashMap<u32, GuestFile>,
    next_fd: u32,
    pub base_dir: PathBuf,
}

struct GuestFile {
    file: File,
    path: String,
    eof: bool,
}

impl GuestFs {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            files: HashMap::new(),
            next_fd: 0x100, // avoid collision with NULL/sentinel values
            base_dir,
        }
    }

    /// Read a UCS-2 LE wide string from guest memory, return UTF-8
    pub fn read_wstring(mem: &Memory, addr: u32) -> String {
        let mut chars = Vec::new();
        let mut off = addr;
        loop {
            let c = mem.read_u16(off);
            if c == 0 {
                break;
            }
            chars.push(c);
            off += 2;
        }
        String::from_utf16_lossy(&chars)
    }

    /// Translate a guest path to a host path
    fn translate_path(&self, guest_path: &str) -> PathBuf {
        let mut p = guest_path.replace('\\', "/");

        // Strip drive prefix like "A:/" or "a:/"
        if p.len() >= 3 && p.as_bytes()[1] == b':' {
            p = p[2..].to_string();
            if p.starts_with('/') {
                p = p[1..].to_string();
            }
        }

        // Strip leading "./"
        if p.starts_with("./") {
            p = p[2..].to_string();
        }

        // Resolve relative to base_dir
        self.base_dir.join(&p)
    }

    pub fn fopen(&mut self, guest_path: &str, mode: &str) -> u32 {
        let host_path = self.translate_path(guest_path);
        eprintln!("[FS] fopen({:?}, {:?}) -> host: {:?}", guest_path, mode, host_path);

        let file = match mode {
            "r" | "rb" => File::open(&host_path),
            "w" | "wb" => File::create(&host_path),
            "a" | "ab" => OpenOptions::new().append(true).create(true).open(&host_path),
            "r+" | "rb+" | "r+b" => OpenOptions::new().read(true).write(true).open(&host_path),
            "w+" | "wb+" | "w+b" => OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&host_path),
            _ => File::open(&host_path),
        };

        match file {
            Ok(f) => {
                let fd = self.next_fd;
                self.next_fd += 1;
                eprintln!("[FS]   -> fd={fd}");
                self.files.insert(fd, GuestFile {
                    file: f,
                    path: guest_path.to_string(),
                    eof: false,
                });
                fd
            }
            Err(e) => {
                eprintln!("[FS]   -> FAILED: {e}");
                0
            }
        }
    }

    pub fn fopen_wide(&mut self, mem: &Memory, wpath_addr: u32, wmode_addr: u32) -> u32 {
        let path = Self::read_wstring(mem, wpath_addr);
        let mode = Self::read_wstring(mem, wmode_addr);
        self.fopen(&path, &mode)
    }

    pub fn fread(&mut self, mem: &mut Memory, buf_addr: u32, size: u32, count: u32, fd: u32) -> u32 {
        let total = (size * count) as usize;
        if let Some(gf) = self.files.get_mut(&fd) {
            let mut tmp = vec![0u8; total];
            match gf.file.read(&mut tmp) {
                Ok(n) => {
                    if n < total {
                        gf.eof = true;
                    }
                    let dest = mem.slice_mut(buf_addr, n);
                    dest.copy_from_slice(&tmp[..n]);
                    if size > 0 { (n / size as usize) as u32 } else { 0 }
                }
                Err(_) => 0,
            }
        } else {
            eprintln!("[FS] fread: bad fd {fd}");
            0
        }
    }

    pub fn fwrite(&mut self, mem: &Memory, buf_addr: u32, size: u32, count: u32, fd: u32) -> u32 {
        let total = (size * count) as usize;
        if let Some(gf) = self.files.get_mut(&fd) {
            let data = mem.slice(buf_addr, total);
            match gf.file.write(data) {
                Ok(n) => {
                    if size > 0 { (n / size as usize) as u32 } else { 0 }
                }
                Err(_) => 0,
            }
        } else {
            eprintln!("[FS] fwrite: bad fd {fd}");
            0
        }
    }

    pub fn fclose(&mut self, fd: u32) -> u32 {
        if self.files.remove(&fd).is_some() {
            0
        } else {
            eprintln!("[FS] fclose: bad fd {fd}");
            !0 // EOF
        }
    }

    pub fn fseek(&mut self, fd: u32, offset: i32, whence: u32) -> u32 {
        if let Some(gf) = self.files.get_mut(&fd) {
            let pos = match whence {
                0 => SeekFrom::Start(offset as u64),
                1 => SeekFrom::Current(offset as i64),
                2 => SeekFrom::End(offset as i64),
                _ => return !0,
            };
            match gf.file.seek(pos) {
                Ok(_) => {
                    gf.eof = false;
                    0
                }
                Err(_) => !0,
            }
        } else {
            eprintln!("[FS] fseek: bad fd {fd}");
            !0
        }
    }

    pub fn ftell(&mut self, fd: u32) -> u32 {
        if let Some(gf) = self.files.get_mut(&fd) {
            match gf.file.stream_position() {
                Ok(pos) => pos as u32,
                Err(_) => !0,
            }
        } else {
            0
        }
    }

    pub fn feof(&self, fd: u32) -> u32 {
        if let Some(gf) = self.files.get(&fd) {
            gf.eof as u32
        } else {
            1
        }
    }

    pub fn ferror(&self, _fd: u32) -> u32 {
        0
    }
}
