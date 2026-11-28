//! Registers KaTeX's embedded TrueType fonts with GPUI's text system, patching
//! OpenType style/weight flags so CoreText distinguishes upright, italic, bold,
//! and bold-italic KaTeX faces accurately.

use std::borrow::Cow;

use gpui::{App, Global};
use katex_native::FONTS;

struct KatexFontsLoaded;
impl Global for KatexFontsLoaded {}

/// Ensures all 20 bundled KaTeX fonts are loaded into `cx.text_system()`.
pub(crate) fn ensure_loaded(cx: &mut App) {
    if cx.has_global::<KatexFontsLoaded>() {
        return;
    }
    let fonts: Vec<Cow<'static, [u8]>> = FONTS
        .iter()
        .map(|file| Cow::Owned(patch_ttf_style(file.data, file.font.bold, file.font.italic)))
        .collect();
    if cx.text_system().add_fonts(fonts).is_ok() {
        cx.set_global(KatexFontsLoaded);
    }
}

/// KaTeX ships its TTF files with `head.macStyle = 0` and `OS/2.fsSelection = REGULAR`
/// even for Italic and BoldItalic faces (relying on CSS `@font-face` on the web).
/// Native font systems read those OpenType flags, so we patch them in-memory and
/// update the table and file checksums before registering with GPUI.
fn patch_ttf_style(data: &[u8], bold: bool, italic: bool) -> Vec<u8> {
    let mut out = data.to_vec();
    if out.len() < 12 {
        return out;
    }
    let num_tables = read_u16(&out, 4) as usize;
    let dir_end = 12 + num_tables * 16;
    if out.len() < dir_end {
        return out;
    }

    let mut head_record: Option<(usize, usize, usize)> = None;
    let mut os2_record: Option<(usize, usize, usize)> = None;

    for i in 0..num_tables {
        let rec = 12 + i * 16;
        let tag = &out[rec..rec + 4];
        let offset = read_u32(&out, rec + 8) as usize;
        let length = read_u32(&out, rec + 12) as usize;
        if offset.checked_add(length).is_none_or(|end| end > out.len()) {
            continue;
        }
        match tag {
            b"head" if length >= 54 => head_record = Some((rec, offset, length)),
            b"OS/2" if length >= 64 => os2_record = Some((rec, offset, length)),
            _ => {}
        }
    }

    let mac_style: u16 = u16::from(bold) | (u16::from(italic) << 1);
    let weight_class: u16 = if bold { 700 } else { 400 };
    let fs_selection: u16 = match (bold, italic) {
        (false, false) => 0x0040, // REGULAR
        (true, false) => 0x0020,  // BOLD
        (false, true) => 0x0001,  // ITALIC
        (true, true) => 0x0021,   // BOLD | ITALIC
    };

    if let Some((rec, offset, length)) = os2_record {
        write_u16(&mut out, offset + 4, weight_class);
        write_u16(&mut out, offset + 62, fs_selection);
        let sum = table_checksum(&out[offset..offset + length]);
        write_u32(&mut out, rec + 4, sum);
    }

    if let Some((rec, offset, length)) = head_record {
        write_u16(&mut out, offset + 44, mac_style);
        // Zero checkSumAdjustment before computing table and whole-font checksums.
        write_u32(&mut out, offset + 8, 0);
        let sum = table_checksum(&out[offset..offset + length]);
        write_u32(&mut out, rec + 4, sum);
        let total = table_checksum(&out);
        let adjustment = 0xB1B0_AFBA_u32.wrapping_sub(total);
        write_u32(&mut out, offset + 8, adjustment);
    }

    out
}

fn read_u16(buf: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([buf[at], buf[at + 1]])
}

fn read_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

fn write_u16(buf: &mut [u8], at: usize, value: u16) {
    buf[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn write_u32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

fn table_checksum(table: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let (chunks, rem) = table.as_chunks::<4>();
    for chunk in chunks {
        sum = sum.wrapping_add(u32::from_be_bytes(*chunk));
    }
    if !rem.is_empty() {
        let mut tail = [0u8; 4];
        tail[..rem.len()].copy_from_slice(rem);
        sum = sum.wrapping_add(u32::from_be_bytes(tail));
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_all_katex_fonts_with_valid_flags_and_checksums() {
        for file in FONTS {
            let patched = patch_ttf_style(file.data, file.font.bold, file.font.italic);
            assert_eq!(patched.len(), file.data.len());
            // Whole-font checksum must equal the OpenType magic constant 0xB1B0AFBA.
            assert_eq!(
                table_checksum(&patched),
                0xB1B0_AFBA,
                "invalid checksum for {}",
                file.postscript_name()
            );
            let num_tables = read_u16(&patched, 4) as usize;
            let mut found_head = false;
            let mut found_os2 = false;
            for i in 0..num_tables {
                let rec = 12 + i * 16;
                let tag = &patched[rec..rec + 4];
                let off = read_u32(&patched, rec + 8) as usize;
                match tag {
                    b"head" => {
                        let expected =
                            u16::from(file.font.bold) | (u16::from(file.font.italic) << 1);
                        assert_eq!(read_u16(&patched, off + 44), expected);
                        found_head = true;
                    }
                    b"OS/2" => {
                        let expected_weight = if file.font.bold { 700 } else { 400 };
                        let expected_sel = match (file.font.bold, file.font.italic) {
                            (false, false) => 0x0040,
                            (true, false) => 0x0020,
                            (false, true) => 0x0001,
                            (true, true) => 0x0021,
                        };
                        assert_eq!(read_u16(&patched, off + 4), expected_weight);
                        assert_eq!(read_u16(&patched, off + 62), expected_sel);
                        found_os2 = true;
                    }
                    _ => {}
                }
            }
            assert!(found_head && found_os2);
        }
    }
}
