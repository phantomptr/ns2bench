use std::collections::BTreeMap;

/// ELF32 little-endian ARM load addresses, including the flash image of .data.
pub fn convert(elf: &[u8]) -> Result<Vec<u8>, String> {
    let bad = || "malformed or unsupported ELF32 ARM image".to_string();
    let word = |offset: usize| -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            elf.get(offset..offset.checked_add(4).ok_or_else(bad)?)
                .ok_or_else(bad)?
                .try_into()
                .map_err(|_| bad())?,
        ))
    };
    let half = |offset: usize| -> Result<u16, String> {
        Ok(u16::from_le_bytes(
            elf.get(offset..offset + 2)
                .ok_or_else(bad)?
                .try_into()
                .map_err(|_| bad())?,
        ))
    };
    if elf.len() < 52 || &elf[..7] != b"\x7fELF\x01\x01\x01" || half(18)? != 40 {
        return Err(bad());
    }
    let headers = word(28)? as usize;
    let stride = half(42)? as usize;
    let count = half(44)? as usize;
    if stride != 32
        || count == 0
        || count > 4096
        || headers
            .checked_add(count * stride)
            .is_none_or(|end| end > elf.len())
    {
        return Err(bad());
    }
    let mut pages: BTreeMap<u32, [u8; 256]> = BTreeMap::new();
    for i in 0..count {
        let h = headers + i * stride;
        let size = word(h + 16)? as usize;
        if word(h)? != 1 || size == 0 {
            continue;
        }
        let offset = word(h + 4)? as usize;
        let address = word(h + 12)?;
        if address < 0x10000000 || address as u64 + size as u64 > 0x11000000 {
            return Err("load segment is outside RP2040 flash".into());
        }
        let data = elf
            .get(offset..offset.checked_add(size).ok_or_else(bad)?)
            .ok_or_else(bad)?;
        for (i, byte) in data.iter().enumerate() {
            let address = address + i as u32;
            pages.entry(address & !255).or_insert([0; 256])[address as usize & 255] = *byte;
        }
    }
    if !pages.contains_key(&0x10000000) {
        return Err("no boot2 block at 0x10000000".into());
    }
    let count = pages.len() as u32;
    let mut output = Vec::with_capacity(pages.len() * 512);
    for (i, (address, page)) in pages.into_iter().enumerate() {
        for word in [
            0x0a324655u32,
            0x9e5d5157,
            0x2000,
            address,
            256,
            i as u32,
            count,
            0xe48bff56,
        ] {
            output.extend_from_slice(&word.to_le_bytes());
        }
        output.extend_from_slice(&page);
        output.extend_from_slice(&[0; 220]);
        output.extend_from_slice(&0x0ab16f30u32.to_le_bytes());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn elf() -> Vec<u8> {
        let mut e = vec![0; 88];
        e[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        e[18..20].copy_from_slice(&40u16.to_le_bytes());
        e[28..32].copy_from_slice(&52u32.to_le_bytes());
        e[42..44].copy_from_slice(&32u16.to_le_bytes());
        e[44..46].copy_from_slice(&1u16.to_le_bytes());
        e[52..56].copy_from_slice(&1u32.to_le_bytes());
        e[56..60].copy_from_slice(&84u32.to_le_bytes());
        e[64..68].copy_from_slice(&0x10000000u32.to_le_bytes());
        e[68..72].copy_from_slice(&4u32.to_le_bytes());
        e[84..].copy_from_slice(&[1, 2, 3, 4]);
        e
    }
    #[test]
    fn uf2_block_layout_and_padding() {
        let out = convert(&elf()).unwrap();
        assert_eq!(out.len(), 512);
        let words: Vec<_> = out[..32]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        assert_eq!(
            words,
            [0x0a324655, 0x9e5d5157, 0x2000, 0x10000000, 256, 0, 1, 0xe48bff56]
        );
        assert_eq!(&out[32..36], &[1, 2, 3, 4]);
        assert!(out[36..508].iter().all(|b| *b == 0));
        assert_eq!(&out[508..], &0x0ab16f30u32.to_le_bytes());
    }
    #[test]
    fn malformed_elf_and_nonflash_segments_are_rejected() {
        for end in 0..88 {
            assert!(convert(&elf()[..end]).is_err(), "length {end}");
        }
        for address in [
            0x0fffffffu32,
            0x20000000,
            0x11000000,
            0x10000100,
            0x10fffffe,
        ] {
            let mut e = elf();
            e[64..68].copy_from_slice(&address.to_le_bytes());
            assert!(convert(&e).is_err(), "address {address:x}");
        }
        let mut e = elf();
        e[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(convert(&e).is_err());
    }
}
