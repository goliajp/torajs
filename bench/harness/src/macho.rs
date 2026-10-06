//! The shape of a compiled artifact, read from its Mach-O load
//! commands: bytes per segment and per section, and the symbol count.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

const MH_MAGIC_64: u32 = 0xfeed_facf;
const LC_SYMTAB: u32 = 0x2;
const LC_SEGMENT_64: u32 = 0x19;

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Shape {
    /// file bytes per segment (`__PAGEZERO`, which has none, is left out)
    pub segments: BTreeMap<String, u64>,
    /// bytes per section, keyed `__SEGMENT,__section`
    pub sections: BTreeMap<String, u64>,
    pub symbols: Option<u64>,
}

pub(crate) fn shape_of(path: &Path) -> Option<Shape> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 32];
    f.read_exact(&mut head).ok()?;
    let sizeofcmds = u32_at(&head, 20)? as usize;
    let mut cmds = vec![0u8; sizeofcmds];
    f.read_exact(&mut cmds).ok()?;
    parse(&head, &cmds)
}

fn parse(head: &[u8], cmds: &[u8]) -> Option<Shape> {
    if u32_at(head, 0)? != MH_MAGIC_64 {
        return None;
    }
    let ncmds = u32_at(head, 16)?;
    let mut shape = Shape::default();
    let mut at = 0usize;
    for _ in 0..ncmds {
        let cmd = u32_at(cmds, at)?;
        let size = u32_at(cmds, at + 4)? as usize;
        let lc = cmds.get(at..at + size)?;
        match cmd {
            LC_SEGMENT_64 => {
                let seg = name_at(lc, 8)?;
                let filesize = u64_at(lc, 48)?;
                if filesize > 0 {
                    *shape.segments.entry(seg.clone()).or_insert(0) += filesize;
                }
                for i in 0..u32_at(lc, 64)? as usize {
                    let s = 72 + i * 80;
                    let key = format!("{},{}", name_at(lc, s + 16)?, name_at(lc, s)?);
                    *shape.sections.entry(key).or_insert(0) += u64_at(lc, s + 40)?;
                }
            }
            LC_SYMTAB => shape.symbols = Some(u64::from(u32_at(lc, 12)?)),
            _ => {}
        }
        at += size;
    }
    Some(shape)
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn name_at(b: &[u8], at: usize) -> Option<String> {
    let raw = b.get(at..at + 16)?;
    let end = raw.iter().position(|&c| c == 0).unwrap_or(16);
    Some(String::from_utf8_lossy(&raw[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_harness_own_binary() {
        let exe = std::env::current_exe().expect("current_exe");
        let Some(shape) = shape_of(&exe) else {
            assert!(!cfg!(target_os = "macos"), "a macOS test binary is Mach-O");
            return;
        };
        assert!(shape.segments.get("__TEXT").copied().unwrap_or(0) > 0, "{shape:?}");
        assert!(shape.sections.get("__TEXT,__text").copied().unwrap_or(0) > 0, "{shape:?}");
        assert!(!shape.segments.contains_key("__PAGEZERO"));
        assert!(shape.symbols.unwrap_or(0) > 0, "{shape:?}");
    }

    #[test]
    fn not_mach_o_is_none() {
        assert_eq!(parse(&[0u8; 32], &[]), None);
    }
}
