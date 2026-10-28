//! Read physical load extents and target type sizes from the linked ARM image.
use super::check_common::Result;
use std::collections::BTreeMap;

pub const RAM_LIMIT: u64 = 160 * 1024;
pub const FLASH_LIMIT: u64 = 512 * 1024;
pub const STACK_RESERVED: u32 = 24 * 1024;
const FLASH: u64 = 0x10000000;
const RAM: u64 = 0x20000000;
const PHYSICAL_FLASH: u64 = 4 * 1024 * 1024;
const PHYSICAL_RAM: u64 = 520 * 1024;

fn bytes(data: &[u8], offset: usize, size: usize) -> Result<&[u8]> {
    data.get(offset..offset.checked_add(size).ok_or("ELF offset overflow")?)
        .ok_or_else(|| "truncated ELF range".into())
}
fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(bytes(data, offset, 2)?.try_into()?))
}
fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes(data, offset, 4)?.try_into()?))
}
fn entry(base: usize, stride: usize, index: usize) -> Result<usize> {
    base.checked_add(stride.checked_mul(index).ok_or("ELF entry overflow")?)
        .ok_or_else(|| "ELF table overflow".into())
}

pub struct Analysis {
    pub flash: u64,
    pub ram: u64,
    pub sections: BTreeMap<String, u32>,
    pub type_sizes: [u32; 7],
    ram_header: usize,
    budget_header: usize,
}

pub fn inspect(data: &[u8]) -> Result<Analysis> {
    if bytes(data, 0, 6)? != b"\x7fELF\x01\x01" || u16_at(data, 18)? != 40 {
        return Err("resource image must be little-endian ARM ELF32".into());
    }
    let program_base = u32_at(data, 28)? as usize;
    let program_stride = u16_at(data, 42)? as usize;
    let program_count = u16_at(data, 44)? as usize;
    if program_stride < 32 {
        return Err("incomplete ELF program entries".into());
    }
    let mut flash_end = FLASH;
    let mut ram_end = RAM;
    let mut ram_header = None;
    for index in 0..program_count {
        let at = entry(program_base, program_stride, index)?;
        if u32_at(data, at)? != 1 {
            continue;
        }
        let virtual_address = u32_at(data, at + 8)? as u64;
        let physical_address = u32_at(data, at + 12)? as u64;
        let file_size = u32_at(data, at + 16)? as u64;
        let memory_size = u32_at(data, at + 20)? as u64;
        if file_size > memory_size {
            return Err("ELF load file exceeds memory extent".into());
        }
        if memory_size > 0 {
            let end = virtual_address + memory_size;
            if (RAM..RAM + PHYSICAL_RAM).contains(&virtual_address) {
                if end > RAM + PHYSICAL_RAM {
                    return Err("load exceeds Pico 2 SRAM".into());
                }
                ram_end = ram_end.max(end);
                ram_header = Some(at);
            } else if virtual_address < FLASH || end > FLASH + PHYSICAL_FLASH {
                return Err("load lies outside Pico 2 memory".into());
            }
        }
        if file_size > 0 {
            if physical_address < FLASH || physical_address + file_size > FLASH + PHYSICAL_FLASH {
                return Err("file-backed load lies outside Pico 2 flash".into());
            }
            bytes(data, u32_at(data, at + 4)? as usize, file_size as usize)?;
            flash_end = flash_end.max(physical_address + file_size);
        }
    }
    let section_base = u32_at(data, 32)? as usize;
    let section_stride = u16_at(data, 46)? as usize;
    let section_count = u16_at(data, 48)? as usize;
    let names_index = u16_at(data, 50)? as usize;
    if section_stride < 40 || names_index >= section_count {
        return Err("invalid ELF section table".into());
    }
    let names_at = entry(section_base, section_stride, names_index)?;
    let names = bytes(
        data,
        u32_at(data, names_at + 16)? as usize,
        u32_at(data, names_at + 20)? as usize,
    )?;
    let mut sections = BTreeMap::new();
    let mut budget = None;
    for index in 0..section_count {
        let at = entry(section_base, section_stride, index)?;
        bytes(data, at, 40)?;
        let start = u32_at(data, at)? as usize;
        let tail = names.get(start..).ok_or("invalid section name offset")?;
        let name = std::str::from_utf8(
            &tail[..tail
                .iter()
                .position(|&b| b == 0)
                .ok_or("unterminated section name")?],
        )?;
        if name.is_empty() {
            continue;
        }
        let size = u32_at(data, at + 20)?;
        if sections.insert(name.to_owned(), size).is_some() {
            return Err("duplicate ELF section name".into());
        }
        if [".bss", ".data", ".stack"].contains(&name) && size > 0 {
            let address = u32_at(data, at + 12)? as u64;
            if u32_at(data, at + 8)? & 2 == 0 || address < RAM || address + size as u64 > ram_end {
                return Err("runtime storage is outside loaded SRAM".into());
            }
        }
        if name == ".runtime_budget" {
            let address = u32_at(data, at + 12)? as u64;
            if size != 28
                || u32_at(data, at + 8)? & 2 == 0
                || address < FLASH
                || address + 28 > flash_end
            {
                return Err("target type-size evidence is incomplete".into());
            }
            let offset = u32_at(data, at + 16)? as usize;
            let mut sizes = [0; 7];
            for (index, size) in sizes.iter_mut().enumerate() {
                *size = u32_at(data, offset + index * 4)?;
            }
            budget = Some((at, sizes));
        }
    }
    let flash = flash_end - FLASH;
    let ram = ram_end - RAM;
    if sections.get(".text").copied().unwrap_or(0) == 0
        || sections.get(".bss").copied().unwrap_or(0) < 64 * 1024
        || sections.get(".stack").copied().unwrap_or(0) < STACK_RESERVED
    {
        return Err("fixture lost code, VM backing or stack reservation".into());
    }
    if flash > FLASH_LIMIT || ram > RAM_LIMIT {
        return Err(format!("resource budget exceeded: flash={flash}, RAM={ram}").into());
    }
    let (budget_header, type_sizes) = budget.ok_or("missing target type-size section")?;
    Ok(Analysis {
        flash,
        ram,
        sections,
        type_sizes,
        ram_header: ram_header.ok_or("missing RAM load")?,
        budget_header,
    })
}

pub fn reject_mutations(data: &[u8], analysis: &Analysis) -> Result<()> {
    for (offset, replacement) in [
        (18, vec![0, 0]),
        (
            analysis.ram_header + 20,
            ((PHYSICAL_RAM + 1) as u32).to_le_bytes().to_vec(),
        ),
        (analysis.budget_header + 20, 0u32.to_le_bytes().to_vec()),
    ] {
        let mut changed = data.to_vec();
        changed
            .get_mut(offset..offset + replacement.len())
            .ok_or("invalid ELF mutation")?
            .copy_from_slice(&replacement);
        if inspect(&changed).is_ok() {
            return Err("ELF verifier accepted a negative control".into());
        }
    }
    Ok(())
}
