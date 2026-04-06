fn read_u32_le(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_u16_le(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn rgb565_to_rgba(val: u16) -> [u8; 4] {
    let r = ((val >> 11) & 0x1f) as u8;
    let g = ((val >> 5) & 0x3f) as u8;
    let b = (val & 0x1f) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
        255,
    ]
}

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA8
}

/// Parse a VID_impl texture from raw bytes.
///
/// Color modes observed:
///   0x52: palette-indexed, bpp=1, palette_size = count * 64 (RGB565 entries + padding)
///   0x91: palette-indexed, bpp=1, palette_size = count * 2 (RGB565 entries)
///   0x12: grayscale, bpp=1, no palette
pub fn parse_vid_impl(data: &[u8]) -> Option<Texture> {
    if data.len() < 12 {
        return None;
    }

    let word0 = read_u32_le(data, 0);
    let mut palette_count = (word0 & 0xff) as usize;
    let color_mode = ((word0 >> 8) & 0xff) as u8;
    let width = read_u32_le(data, 4);
    let height = read_u32_le(data, 8);

    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }

    let pixel_count = (width * height) as usize;
    let mut cursor = 12;

    // Determine bytes per pixel from color_mode bits 4-5
    let bpp_field = ((color_mode & 0x30) >> 4) as usize;
    let bpp = if bpp_field == 0 { 4 } else { bpp_field };

    // Parse palette if present (color_mode & 0xC0 != 0)
    let has_palette = (color_mode & 0xc0) != 0;
    let palette: Option<Vec<[u8; 4]>> = if has_palette {
        if palette_count == 0 {
            palette_count = 256;
        }
        // Palette byte size depends on mode
        let palette_byte_size = if (color_mode & 0xc0) != 0x40 {
            palette_count * 2 // mode 0x80+: RGB565, 2 bytes per entry
        } else {
            palette_count * 64 // mode 0x40: RGB565 + extra data, 64 bytes per entry
        };
        // Read palette_count RGB565 entries (always at 2-byte stride)
        let mut pal = Vec::with_capacity(palette_count);
        for i in 0..palette_count {
            if cursor + i * 2 + 2 > data.len() {
                break;
            }
            let val = read_u16_le(data, cursor + i * 2);
            pal.push(rgb565_to_rgba(val));
        }
        cursor += palette_byte_size;
        // Align to 4 bytes
        if palette_byte_size & 2 != 0 {
            cursor += 2;
        }
        Some(pal)
    } else {
        None
    };

    let mut pixels = Vec::with_capacity(pixel_count * 4);

    if has_palette && bpp == 1 {
        // Palette-indexed, 1 byte per pixel (modes 0x52, 0x91)
        let pal = palette.as_ref()?;
        if cursor + pixel_count > data.len() {
            return None;
        }
        for i in 0..pixel_count {
            let idx = data[cursor + i] as usize;
            let color = pal.get(idx).copied().unwrap_or([255, 0, 255, 255]);
            pixels.extend_from_slice(&color);
        }
    } else if has_palette && bpp == 4 {
        // Palette-indexed, 4bpp (nibble per pixel)
        let pal = palette.as_ref()?;
        let byte_count = (pixel_count + 1) / 2;
        if cursor + byte_count > data.len() {
            return None;
        }
        for i in 0..pixel_count {
            let byte = data[cursor + i / 2];
            let idx = if i & 1 == 0 {
                (byte & 0x0f) as usize
            } else {
                (byte >> 4) as usize
            };
            let color = pal.get(idx).copied().unwrap_or([255, 0, 255, 255]);
            pixels.extend_from_slice(&color);
        }
    } else if !has_palette && bpp == 1 {
        // Grayscale 8-bit (mode 0x12)
        if cursor + pixel_count > data.len() {
            return None;
        }
        for i in 0..pixel_count {
            let v = data[cursor + i];
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    } else if !has_palette && bpp == 2 {
        // Direct RGB565
        if cursor + pixel_count * 2 > data.len() {
            return None;
        }
        for i in 0..pixel_count {
            let val = read_u16_le(data, cursor + i * 2);
            pixels.extend_from_slice(&rgb565_to_rgba(val));
        }
    } else {
        // Unknown — magenta
        for _ in 0..pixel_count {
            pixels.extend_from_slice(&[255, 0, 255, 255]);
        }
    }

    Some(Texture {
        width,
        height,
        pixels,
    })
}

/// Upload a texture to OpenGL. Returns GL texture ID.
pub unsafe fn upload_texture(tex: &Texture) -> u32 {
    let mut id: u32 = 0;
    gl::GenTextures(1, &mut id);
    gl::BindTexture(gl::TEXTURE_2D, id);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as i32);
    gl::TexParameteri(
        gl::TEXTURE_2D,
        gl::TEXTURE_MIN_FILTER,
        gl::LINEAR_MIPMAP_LINEAR as i32,
    );
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
    gl::TexImage2D(
        gl::TEXTURE_2D,
        0,
        gl::RGBA as i32,
        tex.width as i32,
        tex.height as i32,
        0,
        gl::RGBA,
        gl::UNSIGNED_BYTE,
        tex.pixels.as_ptr() as *const _,
    );
    gl::GenerateMipmap(gl::TEXTURE_2D);
    id
}
