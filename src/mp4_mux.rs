//! Pure-Rust streaming ISO BMFF (MP4) muxer for merging separate DASH video and audio tracks.

use crate::process::Cancellation;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::Path;

const MAX_METADATA_BOX_SIZE: u64 = 64 * 1024 * 1024;
const COPY_CHUNK_SIZE: usize = 64 * 1024;

#[derive(Clone, Debug)]
struct BoxSpan {
    offset: usize,
    size: usize,
    header_len: usize,
    fourcc: [u8; 4],
}

impl BoxSpan {
    fn payload_range(&self) -> Range<usize> {
        (self.offset + self.header_len)..(self.offset + self.size)
    }
}

#[derive(Clone, Copy, Debug)]
struct FragmentMeta {
    moof_offset: u64,
    moof_size: u64,
    mdat_offset: u64,
    mdat_size: u64,
}

#[derive(Clone, Debug)]
struct Mp4Source {
    ftyp_payload: Option<Vec<u8>>,
    mvhd_payload: Vec<u8>,
    mvhd_timescale: u32,
    mvhd_duration: u64,
    trak_payload: Vec<u8>,
    mdhd_timescale: u32,
    trex_payload: Option<Vec<u8>>,
    mehd_duration: Option<u64>,
    udta_payload: Option<Vec<u8>>,
    fragments: Vec<FragmentMeta>,
    mdat_spans: Vec<(u64, u64)>,
}

fn fourcc_str(fourcc: &[u8; 4]) -> String {
    fourcc
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            }
        })
        .collect()
}

fn read_u32_be(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

fn read_u64_be(data: &[u8], offset: usize) -> Option<u64> {
    let bytes: [u8; 8] = data.get(offset..offset + 8)?.try_into().ok()?;
    Some(u64::from_be_bytes(bytes))
}

fn write_u32_be(data: &mut [u8], offset: usize, value: u32) -> bool {
    let Some(dst) = data.get_mut(offset..offset + 4) else {
        return false;
    };
    dst.copy_from_slice(&value.to_be_bytes());
    true
}

fn write_u64_be(data: &mut [u8], offset: usize, value: u64) -> bool {
    let Some(dst) = data.get_mut(offset..offset + 8) else {
        return false;
    };
    dst.copy_from_slice(&value.to_be_bytes());
    true
}

fn build_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let payload_len = payload.len() as u64;
    if payload_len + 8 <= u32::MAX as u64 {
        let mut out = Vec::with_capacity(8 + payload.len());
        out.extend_from_slice(&((payload_len as u32) + 8).to_be_bytes());
        out.extend_from_slice(fourcc);
        out.extend_from_slice(payload);
        out
    } else {
        let mut out = Vec::with_capacity(16 + payload.len());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(fourcc);
        out.extend_from_slice(&(payload_len + 16).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }
}

fn write_box_header<W: Write>(
    writer: &mut W,
    fourcc: &[u8; 4],
    payload_len: u64,
) -> Result<u64, String> {
    if let Some(total) = payload_len
        .checked_add(8)
        .filter(|&sum| sum <= u32::MAX as u64)
    {
        writer
            .write_all(&(total as u32).to_be_bytes())
            .map_err(|e| format!("Erro ao escrever cabeçalho MP4: {e}"))?;
        writer
            .write_all(fourcc)
            .map_err(|e| format!("Erro ao escrever tipo de caixa MP4: {e}"))?;
        Ok(8)
    } else {
        let total = payload_len
            .checked_add(16)
            .ok_or_else(|| "Tamanho de caixa MP4 excede u64.".to_owned())?;
        writer
            .write_all(&1u32.to_be_bytes())
            .map_err(|e| format!("Erro ao escrever cabeçalho estendido MP4: {e}"))?;
        writer
            .write_all(fourcc)
            .map_err(|e| format!("Erro ao escrever tipo de caixa MP4: {e}"))?;
        writer
            .write_all(&total.to_be_bytes())
            .map_err(|e| format!("Erro ao escrever tamanho estendido MP4: {e}"))?;
        Ok(16)
    }
}

fn parse_child_boxes(data: &[u8]) -> Result<Vec<BoxSpan>, String> {
    let mut spans = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        if data.len() - pos < 8 {
            return Err("Caixa MP4 truncada ao ler filhos.".to_owned());
        }
        let size32 = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as u64;
        let fourcc: [u8; 4] = data[pos + 4..pos + 8].try_into().unwrap();
        let (size, header_len) = if size32 == 1 {
            if data.len() - pos < 16 {
                return Err(format!(
                    "Caixa MP4 estendida '{}' truncada.",
                    fourcc_str(&fourcc)
                ));
            }
            let size64 = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().unwrap());
            (size64, 16usize)
        } else if size32 == 0 {
            ((data.len() - pos) as u64, 8usize)
        } else {
            (size32, 8usize)
        };
        if size < header_len as u64 {
            return Err(format!(
                "Tamanho inválido para caixa MP4 '{}'.",
                fourcc_str(&fourcc)
            ));
        }
        let size_usize = usize::try_from(size).map_err(|_| {
            format!(
                "Caixa MP4 '{}' excede limite endereçável.",
                fourcc_str(&fourcc)
            )
        })?;
        if pos
            .checked_add(size_usize)
            .is_none_or(|end| end > data.len())
        {
            return Err(format!(
                "Caixa MP4 '{}' ultrapassa os limites do contêiner.",
                fourcc_str(&fourcc)
            ));
        }
        spans.push(BoxSpan {
            offset: pos,
            size: size_usize,
            header_len,
            fourcc,
        });
        pos += size_usize;
    }
    Ok(spans)
}

struct TopBoxHeader {
    start: u64,
    size: u64,
    fourcc: [u8; 4],
    header_len: u64,
}

fn read_box_header_from_reader<R: Read + Seek>(
    reader: &mut R,
    pos: u64,
    parent_end: u64,
) -> Result<Option<TopBoxHeader>, String> {
    if pos >= parent_end {
        return Ok(None);
    }
    if parent_end - pos < 8 {
        return Err("Arquivo MP4 truncado no cabeçalho de caixa.".to_owned());
    }
    reader
        .seek(SeekFrom::Start(pos))
        .map_err(|e| format!("Falha ao posicionar leitura MP4: {e}"))?;
    let mut header = [0u8; 8];
    reader
        .read_exact(&mut header)
        .map_err(|e| format!("Falha ao ler cabeçalho de caixa MP4: {e}"))?;
    let size32 = u32::from_be_bytes(header[0..4].try_into().unwrap()) as u64;
    let fourcc: [u8; 4] = header[4..8].try_into().unwrap();
    let (size, header_len) = if size32 == 1 {
        if parent_end - pos < 16 {
            return Err(format!(
                "Caixa MP4 estendida '{}' truncada.",
                fourcc_str(&fourcc)
            ));
        }
        let mut ext = [0u8; 8];
        reader
            .read_exact(&mut ext)
            .map_err(|e| format!("Falha ao ler tamanho estendido MP4: {e}"))?;
        (u64::from_be_bytes(ext), 16u64)
    } else if size32 == 0 {
        (parent_end - pos, 8u64)
    } else {
        (size32, 8u64)
    };
    if size < header_len {
        return Err(format!(
            "Tamanho inválido para caixa MP4 '{}'.",
            fourcc_str(&fourcc)
        ));
    }
    if pos.checked_add(size).is_none_or(|end| end > parent_end) {
        return Err(format!(
            "Caixa MP4 '{}' ultrapassa o fim do arquivo.",
            fourcc_str(&fourcc)
        ));
    }
    Ok(Some(TopBoxHeader {
        start: pos,
        size,
        fourcc,
        header_len,
    }))
}

fn read_box_payload(
    reader: &mut BufReader<File>,
    payload_offset: u64,
    payload_len: u64,
    fourcc: &[u8; 4],
) -> Result<Vec<u8>, String> {
    if payload_len > MAX_METADATA_BOX_SIZE {
        return Err(format!(
            "Caixa de metadados MP4 '{}' excede o limite de 64 MiB.",
            fourcc_str(fourcc)
        ));
    }
    reader
        .seek(SeekFrom::Start(payload_offset))
        .map_err(|e| format!("Erro ao posicionar caixa '{}': {e}", fourcc_str(fourcc)))?;
    let mut buf = vec![0u8; payload_len as usize];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("Erro ao ler caixa '{}': {e}", fourcc_str(fourcc)))?;
    Ok(buf)
}

fn parse_mvhd(payload: &[u8]) -> Result<(u32, u64), String> {
    let version = *payload
        .first()
        .ok_or_else(|| "Caixa 'mvhd' vazia.".to_owned())?;
    match version {
        0 => {
            if payload.len() < 100 {
                return Err("Caixa 'mvhd' v0 incompleta.".to_owned());
            }
            let timescale = read_u32_be(payload, 12).unwrap_or(1000).max(1);
            let duration = read_u32_be(payload, 16).unwrap_or(0) as u64;
            Ok((timescale, duration))
        }
        1 => {
            if payload.len() < 112 {
                return Err("Caixa 'mvhd' v1 incompleta.".to_owned());
            }
            let timescale = read_u32_be(payload, 20).unwrap_or(1000).max(1);
            let duration = read_u64_be(payload, 24).unwrap_or(0);
            Ok((timescale, duration))
        }
        other => Err(format!("Versão não suportada de 'mvhd': {other}")),
    }
}

fn parse_mdhd_timescale(payload: &[u8]) -> Result<u32, String> {
    let version = *payload
        .first()
        .ok_or_else(|| "Caixa 'mdhd' vazia.".to_owned())?;
    let ts = match version {
        0 => read_u32_be(payload, 12).ok_or_else(|| "Caixa 'mdhd' v0 incompleta.".to_owned())?,
        1 => read_u32_be(payload, 20).ok_or_else(|| "Caixa 'mdhd' v1 incompleta.".to_owned())?,
        other => return Err(format!("Versão não suportada de 'mdhd': {other}")),
    };
    Ok(ts.max(1))
}

fn parse_mehd_duration(payload: &[u8]) -> Result<u64, String> {
    let version = *payload
        .first()
        .ok_or_else(|| "Caixa 'mehd' vazia.".to_owned())?;
    match version {
        0 => read_u32_be(payload, 4)
            .map(|v| v as u64)
            .ok_or_else(|| "Caixa 'mehd' v0 incompleta.".to_owned()),
        1 => read_u64_be(payload, 4).ok_or_else(|| "Caixa 'mehd' v1 incompleta.".to_owned()),
        other => Err(format!("Versão não suportada de 'mehd': {other}")),
    }
}

fn extract_trak_mdhd_timescale(trak_payload: &[u8]) -> Result<u32, String> {
    let mut has_tkhd = false;
    let mut mdhd_timescale = None;
    for child in parse_child_boxes(trak_payload)? {
        if &child.fourcc == b"tkhd" {
            has_tkhd = true;
        } else if &child.fourcc == b"mdia" {
            for mdia_child in parse_child_boxes(&trak_payload[child.payload_range()])? {
                if &mdia_child.fourcc == b"mdhd" {
                    let range = child.offset + child.header_len;
                    let start = range + mdia_child.offset + mdia_child.header_len;
                    let end = range + mdia_child.offset + mdia_child.size;
                    mdhd_timescale = Some(parse_mdhd_timescale(&trak_payload[start..end])?);
                }
            }
        }
    }
    if !has_tkhd {
        return Err("Trilha MP4 sem caixa 'tkhd'.".to_owned());
    }
    mdhd_timescale.ok_or_else(|| "Trilha MP4 sem caixa 'mdhd'.".to_owned())
}

fn scan_mp4(path: &Path) -> Result<Mp4Source, String> {
    let file = File::open(path)
        .map_err(|e| format!("Não foi possível abrir MP4 '{}': {e}", path.display()))?;
    let file_len = file
        .metadata()
        .map_err(|e| format!("Erro ao consultar tamanho de MP4: {e}"))?
        .len();
    let mut reader = BufReader::new(file);

    let mut ftyp_payload = None;
    let mut moov_payload = None;
    let mut fragments = Vec::new();
    let mut mdat_spans = Vec::new();
    let mut pending_moof: Option<(u64, u64)> = None;

    let mut pos = 0u64;
    while let Some(TopBoxHeader {
        start: box_start,
        size: box_size,
        fourcc,
        header_len,
    }) = read_box_header_from_reader(&mut reader, pos, file_len)?
    {
        let payload_offset = box_start + header_len;
        let payload_len = box_size - header_len;
        match &fourcc {
            b"ftyp" => {
                if ftyp_payload.is_none() {
                    ftyp_payload = Some(read_box_payload(
                        &mut reader,
                        payload_offset,
                        payload_len,
                        &fourcc,
                    )?);
                }
            }
            b"moov" => {
                if moov_payload.is_none() {
                    moov_payload = Some(read_box_payload(
                        &mut reader,
                        payload_offset,
                        payload_len,
                        &fourcc,
                    )?);
                }
            }
            b"moof" => {
                if pending_moof.is_some() {
                    return Err("MP4 inválido: 'moof' consecutivo sem 'mdat'.".to_owned());
                }
                pending_moof = Some((box_start, box_size));
            }
            b"mdat" => {
                mdat_spans.push((payload_offset, payload_len));
                if let Some((moof_offset, moof_size)) = pending_moof.take() {
                    fragments.push(FragmentMeta {
                        moof_offset,
                        moof_size,
                        mdat_offset: box_start,
                        mdat_size: box_size,
                    });
                }
            }
            _ => {}
        }
        pos = box_start + box_size;
    }

    if pending_moof.is_some() {
        return Err("MP4 inválido: 'moof' final sem 'mdat' correspondente.".to_owned());
    }

    let moov_bytes =
        moov_payload.ok_or_else(|| "MP4 inválido: caixa 'moov' ausente.".to_owned())?;
    let mut mvhd_payload = None;
    let mut trak_payload = None;
    let mut trex_payload = None;
    let mut mehd_duration = None;
    let mut udta_payload = None;

    for child in parse_child_boxes(&moov_bytes)? {
        let slice = &moov_bytes[child.payload_range()];
        match &child.fourcc {
            b"mvhd" if mvhd_payload.is_none() => {
                mvhd_payload = Some(slice.to_vec());
            }
            b"trak" if trak_payload.is_none() => {
                trak_payload = Some(slice.to_vec());
            }
            b"mvex" => {
                for mvex_child in parse_child_boxes(slice)? {
                    let mvex_slice = &slice[mvex_child.payload_range()];
                    if &mvex_child.fourcc == b"trex" && trex_payload.is_none() {
                        trex_payload = Some(mvex_slice.to_vec());
                    } else if &mvex_child.fourcc == b"mehd" && mehd_duration.is_none() {
                        mehd_duration = parse_mehd_duration(mvex_slice).ok();
                    }
                }
            }
            b"udta" if udta_payload.is_none() => {
                udta_payload = Some(slice.to_vec());
            }
            _ => {}
        }
    }

    let mvhd_payload =
        mvhd_payload.ok_or_else(|| "MP4 inválido: caixa 'mvhd' ausente.".to_owned())?;
    let (mvhd_timescale, mvhd_duration) = parse_mvhd(&mvhd_payload)?;
    let trak_payload =
        trak_payload.ok_or_else(|| "MP4 inválido: nenhuma trilha encontrada.".to_owned())?;
    let mdhd_timescale = extract_trak_mdhd_timescale(&trak_payload)?;

    Ok(Mp4Source {
        ftyp_payload,
        mvhd_payload,
        mvhd_timescale,
        mvhd_duration,
        trak_payload,
        mdhd_timescale,
        trex_payload,
        mehd_duration,
        udta_payload,
        fragments,
        mdat_spans,
    })
}

fn rescale_u64(value: u64, from_ts: u32, to_ts: u32) -> u64 {
    if from_ts == 0 || to_ts == 0 || from_ts == to_ts {
        return value;
    }
    let scaled = (value as u128).saturating_mul(to_ts as u128) / (from_ts as u128);
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

fn build_ftyp_box(video_ftyp: Option<&[u8]>, audio_ftyp: Option<&[u8]>) -> Vec<u8> {
    let mut brands: Vec<[u8; 4]> = Vec::new();
    let mut seen: HashSet<[u8; 4]> = HashSet::new();
    let mut push_brand = |b: [u8; 4]| {
        if seen.insert(b) {
            brands.push(b);
        }
    };

    for default_brand in [*b"isom", *b"iso5", *b"iso6", *b"mp41"] {
        push_brand(default_brand);
    }
    for src in [video_ftyp, audio_ftyp].into_iter().flatten() {
        if src.len() >= 4 {
            push_brand(src[0..4].try_into().unwrap());
        }
        let mut idx = 8usize;
        while idx + 4 <= src.len() {
            push_brand(src[idx..idx + 4].try_into().unwrap());
            idx += 4;
        }
    }

    let mut payload = Vec::with_capacity(8 + brands.len() * 4);
    payload.extend_from_slice(b"isom");
    payload.extend_from_slice(&512u32.to_be_bytes());
    for brand in brands {
        payload.extend_from_slice(&brand);
    }
    build_box(b"ftyp", &payload)
}

fn patch_mvhd(mvhd_payload: &[u8], duration: u64, next_track_id: u32) -> Result<Vec<u8>, String> {
    let mut out = mvhd_payload.to_vec();
    let version = *out
        .first()
        .ok_or_else(|| "Caixa 'mvhd' vazia.".to_owned())?;
    match version {
        0 => {
            if out.len() < 100 {
                return Err("Caixa 'mvhd' v0 incompleta.".to_owned());
            }
            let dur32 = u32::try_from(duration).unwrap_or(u32::MAX);
            write_u32_be(&mut out, 16, dur32);
            write_u32_be(&mut out, 96, next_track_id);
        }
        1 => {
            if out.len() < 112 {
                return Err("Caixa 'mvhd' v1 incompleta.".to_owned());
            }
            write_u64_be(&mut out, 24, duration);
            write_u32_be(&mut out, 108, next_track_id);
        }
        other => return Err(format!("Versão não suportada de 'mvhd': {other}")),
    }
    Ok(out)
}

fn patch_tkhd_in_place(
    tkhd_payload: &mut [u8],
    track_id: u32,
    rescale_ts: Option<(u32, u32)>,
) -> Result<(), String> {
    let version = *tkhd_payload
        .first()
        .ok_or_else(|| "Caixa 'tkhd' vazia.".to_owned())?;
    match version {
        0 => {
            if tkhd_payload.len() < 24 {
                return Err("Caixa 'tkhd' v0 incompleta.".to_owned());
            }
            write_u32_be(tkhd_payload, 12, track_id);
            if let Some((from_ts, to_ts)) = rescale_ts
                && let Some(old_dur) = read_u32_be(tkhd_payload, 20)
            {
                let new_dur =
                    u32::try_from(rescale_u64(old_dur as u64, from_ts, to_ts)).unwrap_or(u32::MAX);
                write_u32_be(tkhd_payload, 20, new_dur);
            }
        }
        1 => {
            if tkhd_payload.len() < 36 {
                return Err("Caixa 'tkhd' v1 incompleta.".to_owned());
            }
            write_u32_be(tkhd_payload, 20, track_id);
            if let Some((from_ts, to_ts)) = rescale_ts
                && let Some(old_dur) = read_u64_be(tkhd_payload, 28)
            {
                let new_dur = rescale_u64(old_dur, from_ts, to_ts);
                write_u64_be(tkhd_payload, 28, new_dur);
            }
        }
        other => return Err(format!("Versão não suportada de 'tkhd': {other}")),
    }
    Ok(())
}

fn patch_elst_in_place(elst_payload: &mut [u8], from_ts: u32, to_ts: u32) -> Result<(), String> {
    if elst_payload.len() < 8 {
        return Ok(());
    }
    let version = elst_payload[0];
    let entry_count = read_u32_be(elst_payload, 4).unwrap_or(0) as usize;
    match version {
        0 => {
            for i in 0..entry_count {
                let entry_off = 8 + i * 12;
                if entry_off + 12 > elst_payload.len() {
                    break;
                }
                if let Some(seg_dur) = read_u32_be(elst_payload, entry_off) {
                    let scaled = u32::try_from(rescale_u64(seg_dur as u64, from_ts, to_ts))
                        .unwrap_or(u32::MAX);
                    write_u32_be(elst_payload, entry_off, scaled);
                }
            }
        }
        1 => {
            for i in 0..entry_count {
                let entry_off = 8 + i * 20;
                if entry_off + 20 > elst_payload.len() {
                    break;
                }
                if let Some(seg_dur) = read_u64_be(elst_payload, entry_off) {
                    let scaled = rescale_u64(seg_dur, from_ts, to_ts);
                    write_u64_be(elst_payload, entry_off, scaled);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn patch_trak_metadata(
    trak_payload: &[u8],
    track_id: u32,
    rescale_ts: Option<(u32, u32)>,
) -> Result<Vec<u8>, String> {
    let mut out = trak_payload.to_vec();
    for child in parse_child_boxes(&out)? {
        let range = child.payload_range();
        if &child.fourcc == b"tkhd" {
            patch_tkhd_in_place(&mut out[range], track_id, rescale_ts)?;
        } else if &child.fourcc == b"edts"
            && let Some((from_ts, to_ts)) = rescale_ts
        {
            let edts_base = range.start;
            let edts_children = parse_child_boxes(&out[range])?;
            for edts_child in edts_children {
                if &edts_child.fourcc == b"elst" {
                    let start = edts_base + edts_child.offset + edts_child.header_len;
                    let end = edts_base + edts_child.offset + edts_child.size;
                    patch_elst_in_place(&mut out[start..end], from_ts, to_ts)?;
                }
            }
        }
    }
    Ok(out)
}

fn build_trex(trex_payload: Option<&[u8]>, track_id: u32) -> Vec<u8> {
    let mut payload = match trex_payload {
        Some(existing) if existing.len() >= 24 => existing.to_vec(),
        _ => {
            let mut default = vec![0u8; 24];
            write_u32_be(&mut default, 8, 1); // default_sample_description_index = 1
            default
        }
    };
    write_u32_be(&mut payload, 4, track_id);
    build_box(b"trex", &payload)
}

fn build_mehd(duration: u64) -> Vec<u8> {
    if let Ok(dur32) = u32::try_from(duration) {
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&dur32.to_be_bytes());
        build_box(b"mehd", &payload)
    } else {
        let mut payload = Vec::with_capacity(12);
        payload.extend_from_slice(&[1, 0, 0, 0]);
        payload.extend_from_slice(&duration.to_be_bytes());
        build_box(b"mehd", &payload)
    }
}

fn read_moof_bytes(file: &mut File, meta: FragmentMeta) -> Result<Vec<u8>, String> {
    if meta.moof_size > MAX_METADATA_BOX_SIZE {
        return Err("Caixa 'moof' excede o limite de 64 MiB.".to_owned());
    }
    file.seek(SeekFrom::Start(meta.moof_offset))
        .map_err(|e| format!("Falha ao posicionar 'moof': {e}"))?;
    let mut buf = vec![0u8; meta.moof_size as usize];
    file.read_exact(&mut buf)
        .map_err(|e| format!("Falha ao ler 'moof': {e}"))?;
    Ok(buf)
}

fn parse_moof_tfdt(moof_bytes: &[u8]) -> Result<u64, String> {
    let top = parse_child_boxes(moof_bytes)?;
    let Some(moof_box) = top.first() else {
        return Ok(0);
    };
    let moof_payload = &moof_bytes[moof_box.payload_range()];
    for child in parse_child_boxes(moof_payload)? {
        if &child.fourcc == b"traf" {
            for traf_child in parse_child_boxes(&moof_payload[child.payload_range()])? {
                if &traf_child.fourcc == b"tfdt" {
                    let start =
                        child.offset + child.header_len + traf_child.offset + traf_child.header_len;
                    let end = child.offset + child.header_len + traf_child.offset + traf_child.size;
                    let tfdt = &moof_payload[start..end];
                    let Some(&version) = tfdt.first() else {
                        return Ok(0);
                    };
                    return Ok(match version {
                        0 => read_u32_be(tfdt, 4).unwrap_or(0) as u64,
                        1 => read_u64_be(tfdt, 4).unwrap_or(0),
                        _ => 0,
                    });
                }
            }
        }
    }
    Ok(0)
}

fn patch_moof_in_place(
    moof_bytes: &mut [u8],
    seq_num: u32,
    target_track_id: u32,
    old_moof_offset: u64,
    new_moof_offset: u64,
) -> Result<(), String> {
    let top = parse_child_boxes(moof_bytes)?;
    let moof_box = top
        .first()
        .ok_or_else(|| "Caixa 'moof' vazia.".to_owned())?;
    let moof_payload_start = moof_box.offset + moof_box.header_len;
    let moof_payload_end = moof_box.offset + moof_box.size;
    let moof_children = parse_child_boxes(&moof_bytes[moof_payload_start..moof_payload_end])?;
    for child in moof_children {
        let abs_payload_start = moof_payload_start + child.offset + child.header_len;
        let abs_payload_end = moof_payload_start + child.offset + child.size;
        if &child.fourcc == b"mfhd" {
            let mfhd = &mut moof_bytes[abs_payload_start..abs_payload_end];
            if mfhd.len() >= 8 {
                write_u32_be(mfhd, 4, seq_num);
            }
        } else if &child.fourcc == b"traf" {
            let traf_children = parse_child_boxes(&moof_bytes[abs_payload_start..abs_payload_end])?;
            for traf_child in traf_children {
                if &traf_child.fourcc == b"tfhd" {
                    let tfhd_start = abs_payload_start + traf_child.offset + traf_child.header_len;
                    let tfhd_end = abs_payload_start + traf_child.offset + traf_child.size;
                    let tfhd = &mut moof_bytes[tfhd_start..tfhd_end];
                    if tfhd.len() < 8 {
                        return Err("Caixa 'tfhd' incompleta.".to_owned());
                    }
                    let flags = u32::from_be_bytes([0, tfhd[1], tfhd[2], tfhd[3]]);
                    write_u32_be(tfhd, 4, target_track_id);
                    if (flags & 0x0000_0001) != 0 && tfhd.len() >= 16 {
                        let old_base = read_u64_be(tfhd, 8).unwrap_or(0);
                        let delta = (new_moof_offset as i128) - (old_moof_offset as i128);
                        let adjusted = (old_base as i128).saturating_add(delta);
                        let new_base = u64::try_from(adjusted).map_err(|_| {
                            "Deslocamento inválido em 'tfhd.base_data_offset'.".to_owned()
                        })?;
                        write_u64_be(tfhd, 8, new_base);
                    }
                }
            }
        }
    }
    Ok(())
}

fn build_tfra_box(track_id: u32, entries: &[(u64, u64)]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16 + entries.len() * 19);
    payload.extend_from_slice(&[1, 0, 0, 0]); // version = 1, flags = 0
    payload.extend_from_slice(&track_id.to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes()); // 1 byte each for traf, trun, sample nums
    let count = u32::try_from(entries.len()).unwrap_or(u32::MAX);
    payload.extend_from_slice(&count.to_be_bytes());
    for &(time, moof_offset) in entries.iter().take(count as usize) {
        payload.extend_from_slice(&time.to_be_bytes());
        payload.extend_from_slice(&moof_offset.to_be_bytes());
        payload.push(1); // traf_number
        payload.push(1); // trun_number
        payload.push(1); // sample_number
    }
    build_box(b"tfra", &payload)
}

fn build_mfra_box(video_entries: &[(u64, u64)], audio_entries: &[(u64, u64)]) -> Vec<u8> {
    let video_tfra = build_tfra_box(1, video_entries);
    let audio_tfra = build_tfra_box(2, audio_entries);
    let total_mfra_size = 8 + video_tfra.len() + audio_tfra.len() + 16;
    let total_u32 = u32::try_from(total_mfra_size).unwrap_or(0);
    let mut mfro_payload = Vec::with_capacity(8);
    mfro_payload.extend_from_slice(&0u32.to_be_bytes());
    mfro_payload.extend_from_slice(&total_u32.to_be_bytes());
    let mfro = build_box(b"mfro", &mfro_payload);

    let mut payload = Vec::with_capacity(video_tfra.len() + audio_tfra.len() + mfro.len());
    payload.extend_from_slice(&video_tfra);
    payload.extend_from_slice(&audio_tfra);
    payload.extend_from_slice(&mfro);
    build_box(b"mfra", &payload)
}

fn copy_exact_range<W: Write>(
    src: &mut File,
    src_offset: u64,
    mut remaining: u64,
    dst: &mut W,
    buf: &mut [u8],
    cancel: &Cancellation,
) -> Result<(), String> {
    src.seek(SeekFrom::Start(src_offset))
        .map_err(|e| format!("Falha ao posicionar fluxo MP4: {e}"))?;
    while remaining > 0 {
        if cancel.cancelled() {
            return Err("Download cancelado.".to_owned());
        }
        let to_read = remaining.min(buf.len() as u64) as usize;
        let read = src
            .read(&mut buf[..to_read])
            .map_err(|e| format!("Falha ao ler dados de mídia MP4: {e}"))?;
        if read == 0 {
            return Err("Fim inesperado ao copiar dados MP4.".to_owned());
        }
        dst.write_all(&buf[..read])
            .map_err(|e| format!("Falha ao gravar arquivo MP4 combinado: {e}"))?;
        remaining -= read as u64;
    }
    Ok(())
}

fn mux_fragmented(
    video_path: &Path,
    audio_path: &Path,
    video: &Mp4Source,
    audio: &Mp4Source,
    output_path: &Path,
    cancel: &Cancellation,
    mut on_progress: impl FnMut(f32),
) -> Result<(), String> {
    let ftyp = build_ftyp_box(video.ftyp_payload.as_deref(), audio.ftyp_payload.as_deref());
    let out_mvhd_ts = video.mvhd_timescale.max(1);
    let audio_mvhd_dur_rescaled = rescale_u64(
        audio.mvhd_duration,
        audio.mvhd_timescale.max(1),
        out_mvhd_ts,
    );
    let out_mvhd_dur = video.mvhd_duration.max(audio_mvhd_dur_rescaled);
    let mvhd_payload = patch_mvhd(&video.mvhd_payload, out_mvhd_dur, 3)?;
    let video_trak_payload = patch_trak_metadata(&video.trak_payload, 1, None)?;
    let audio_trak_payload = patch_trak_metadata(
        &audio.trak_payload,
        2,
        Some((audio.mvhd_timescale.max(1), out_mvhd_ts)),
    )?;

    let mut mvex_payload = Vec::new();
    let audio_mehd_rescaled = audio
        .mehd_duration
        .map(|d| rescale_u64(d, audio.mvhd_timescale.max(1), out_mvhd_ts));
    let merged_mehd = match (video.mehd_duration, audio_mehd_rescaled) {
        (Some(v), Some(a)) => Some(v.max(a)),
        (Some(v), None) => Some(v),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    };
    if let Some(mehd_dur) = merged_mehd {
        mvex_payload.extend_from_slice(&build_mehd(mehd_dur));
    }
    mvex_payload.extend_from_slice(&build_trex(video.trex_payload.as_deref(), 1));
    mvex_payload.extend_from_slice(&build_trex(audio.trex_payload.as_deref(), 2));

    let mut moov_payload = Vec::new();
    moov_payload.extend_from_slice(&build_box(b"mvhd", &mvhd_payload));
    moov_payload.extend_from_slice(&build_box(b"trak", &video_trak_payload));
    moov_payload.extend_from_slice(&build_box(b"trak", &audio_trak_payload));
    moov_payload.extend_from_slice(&build_box(b"mvex", &mvex_payload));
    if let Some(udta) = &video.udta_payload {
        moov_payload.extend_from_slice(&build_box(b"udta", udta));
    }
    let moov = build_box(b"moov", &moov_payload);

    let out_file = File::create(output_path)
        .map_err(|e| format!("Não foi possível criar arquivo MP4 combinado: {e}"))?;
    let mut writer = BufWriter::with_capacity(COPY_CHUNK_SIZE, out_file);

    writer
        .write_all(&ftyp)
        .map_err(|e| format!("Falha ao gravar 'ftyp': {e}"))?;
    writer
        .write_all(&moov)
        .map_err(|e| format!("Falha ao gravar 'moov': {e}"))?;
    let mut written_offset = (ftyp.len() + moov.len()) as u64;

    let mut video_file = File::open(video_path)
        .map_err(|e| format!("Não foi possível abrir trilha de vídeo: {e}"))?;
    let mut audio_file = File::open(audio_path)
        .map_err(|e| format!("Não foi possível abrir trilha de áudio: {e}"))?;

    let total_bytes: u64 = video
        .fragments
        .iter()
        .chain(audio.fragments.iter())
        .map(|f| f.moof_size + f.mdat_size)
        .sum::<u64>()
        .max(1);
    let mut copied_bytes = 0u64;
    let mut last_pct = -1.0f32;
    let mut copy_buf = vec![0u8; COPY_CHUNK_SIZE];

    let mut vi = 0usize;
    let mut ai = 0usize;
    let mut next_v: Option<(Vec<u8>, u64)> = None;
    let mut next_a: Option<(Vec<u8>, u64)> = None;
    let mut seq = 1u32;
    let mut video_tfra = Vec::with_capacity(video.fragments.len());
    let mut audio_tfra = Vec::with_capacity(audio.fragments.len());

    while vi < video.fragments.len() || ai < audio.fragments.len() {
        if cancel.cancelled() {
            return Err("Download cancelado.".to_owned());
        }
        if next_v.is_none() && vi < video.fragments.len() {
            let bytes = read_moof_bytes(&mut video_file, video.fragments[vi])?;
            let tfdt = parse_moof_tfdt(&bytes)?;
            next_v = Some((bytes, tfdt));
        }
        if next_a.is_none() && ai < audio.fragments.len() {
            let bytes = read_moof_bytes(&mut audio_file, audio.fragments[ai])?;
            let tfdt = parse_moof_tfdt(&bytes)?;
            next_a = Some((bytes, tfdt));
        }

        let pick_video = match (&next_v, &next_a) {
            (Some((_, v_tfdt)), Some((_, a_tfdt))) => {
                let v_scaled = (*v_tfdt as u128).saturating_mul(audio.mdhd_timescale as u128);
                let a_scaled = (*a_tfdt as u128).saturating_mul(video.mdhd_timescale as u128);
                v_scaled <= a_scaled
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };

        if pick_video {
            let (mut moof_bytes, tfdt) = next_v.take().unwrap();
            let frag = video.fragments[vi];
            vi += 1;
            let moof_out_offset = written_offset;
            patch_moof_in_place(&mut moof_bytes, seq, 1, frag.moof_offset, moof_out_offset)?;
            seq = seq.saturating_add(1);
            video_tfra.push((tfdt, moof_out_offset));
            writer
                .write_all(&moof_bytes)
                .map_err(|e| format!("Falha ao gravar fragmento de vídeo: {e}"))?;
            copy_exact_range(
                &mut video_file,
                frag.mdat_offset,
                frag.mdat_size,
                &mut writer,
                &mut copy_buf,
                cancel,
            )?;
            let frag_bytes = frag.moof_size + frag.mdat_size;
            written_offset += frag_bytes;
            copied_bytes += frag_bytes;
        } else {
            let (mut moof_bytes, tfdt) = next_a.take().unwrap();
            let frag = audio.fragments[ai];
            ai += 1;
            let moof_out_offset = written_offset;
            patch_moof_in_place(&mut moof_bytes, seq, 2, frag.moof_offset, moof_out_offset)?;
            seq = seq.saturating_add(1);
            audio_tfra.push((tfdt, moof_out_offset));
            writer
                .write_all(&moof_bytes)
                .map_err(|e| format!("Falha ao gravar fragmento de áudio: {e}"))?;
            copy_exact_range(
                &mut audio_file,
                frag.mdat_offset,
                frag.mdat_size,
                &mut writer,
                &mut copy_buf,
                cancel,
            )?;
            let frag_bytes = frag.moof_size + frag.mdat_size;
            written_offset += frag_bytes;
            copied_bytes += frag_bytes;
        }

        let pct = ((copied_bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0) as f32;
        if pct - last_pct >= 0.5 || copied_bytes >= total_bytes {
            last_pct = pct;
            on_progress(pct);
        }
    }

    let mfra = build_mfra_box(&video_tfra, &audio_tfra);
    writer
        .write_all(&mfra)
        .map_err(|e| format!("Falha ao gravar índice 'mfra': {e}"))?;
    writer
        .flush()
        .map_err(|e| format!("Falha ao descarregar MP4 combinado: {e}"))?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|e| format!("Falha ao sincronizar MP4 combinado: {e}"))?;
    on_progress(100.0);
    Ok(())
}

fn map_chunk_offset(old_offset: u64, span_map: &[(u64, u64, u64)]) -> Option<u64> {
    for &(src_start, len, dst_start) in span_map {
        if old_offset >= src_start && old_offset < src_start.saturating_add(len) {
            return Some(dst_start + (old_offset - src_start));
        }
    }
    let &(first_src, _, first_dst) = span_map.first()?;
    let delta = (first_dst as i128) - (first_src as i128);
    u64::try_from((old_offset as i128).saturating_add(delta)).ok()
}

fn rewrite_chunk_offset_box(
    fourcc: &[u8; 4],
    payload: &[u8],
    span_map: &[(u64, u64, u64)],
) -> Result<Vec<u8>, String> {
    if payload.len() < 8 {
        return Err("Tabela de deslocamento MP4 incompleta.".to_owned());
    }
    let count = read_u32_be(payload, 4).unwrap_or(0) as usize;
    let mut offsets = Vec::with_capacity(count);
    if fourcc == b"stco" {
        if payload.len() < 8 + count * 4 {
            return Err("Caixa 'stco' truncada.".to_owned());
        }
        for i in 0..count {
            let old = read_u32_be(payload, 8 + i * 4).unwrap() as u64;
            let mapped = map_chunk_offset(old, span_map)
                .ok_or_else(|| "Falha ao remapear deslocamento 'stco'.".to_owned())?;
            offsets.push(mapped);
        }
    } else {
        if payload.len() < 8 + count * 8 {
            return Err("Caixa 'co64' truncada.".to_owned());
        }
        for i in 0..count {
            let old = read_u64_be(payload, 8 + i * 8).unwrap();
            let mapped = map_chunk_offset(old, span_map)
                .ok_or_else(|| "Falha ao remapear deslocamento 'co64'.".to_owned())?;
            offsets.push(mapped);
        }
    }

    let fits_u32 = offsets.iter().all(|&off| off <= u32::MAX as u64);
    if fits_u32 {
        let mut new_payload = Vec::with_capacity(8 + offsets.len() * 4);
        new_payload.extend_from_slice(&0u32.to_be_bytes());
        new_payload.extend_from_slice(&(offsets.len() as u32).to_be_bytes());
        for off in offsets {
            new_payload.extend_from_slice(&(off as u32).to_be_bytes());
        }
        Ok(build_box(b"stco", &new_payload))
    } else {
        let mut new_payload = Vec::with_capacity(8 + offsets.len() * 8);
        new_payload.extend_from_slice(&0u32.to_be_bytes());
        new_payload.extend_from_slice(&(offsets.len() as u32).to_be_bytes());
        for off in offsets {
            new_payload.extend_from_slice(&off.to_be_bytes());
        }
        Ok(build_box(b"co64", &new_payload))
    }
}

fn rebuild_stbl_with_offsets(
    stbl_payload: &[u8],
    span_map: &[(u64, u64, u64)],
) -> Result<Vec<u8>, String> {
    let mut rebuilt = Vec::with_capacity(stbl_payload.len());
    for child in parse_child_boxes(stbl_payload)? {
        let slice = &stbl_payload[child.payload_range()];
        if &child.fourcc == b"stco" || &child.fourcc == b"co64" {
            rebuilt.extend_from_slice(&rewrite_chunk_offset_box(&child.fourcc, slice, span_map)?);
        } else {
            rebuilt.extend_from_slice(&build_box(&child.fourcc, slice));
        }
    }
    Ok(build_box(b"stbl", &rebuilt))
}

fn rebuild_minf_with_offsets(
    minf_payload: &[u8],
    span_map: &[(u64, u64, u64)],
) -> Result<Vec<u8>, String> {
    let mut rebuilt = Vec::with_capacity(minf_payload.len());
    for child in parse_child_boxes(minf_payload)? {
        let slice = &minf_payload[child.payload_range()];
        if &child.fourcc == b"stbl" {
            rebuilt.extend_from_slice(&rebuild_stbl_with_offsets(slice, span_map)?);
        } else {
            rebuilt.extend_from_slice(&build_box(&child.fourcc, slice));
        }
    }
    Ok(build_box(b"minf", &rebuilt))
}

fn rebuild_mdia_with_offsets(
    mdia_payload: &[u8],
    span_map: &[(u64, u64, u64)],
) -> Result<Vec<u8>, String> {
    let mut rebuilt = Vec::with_capacity(mdia_payload.len());
    for child in parse_child_boxes(mdia_payload)? {
        let slice = &mdia_payload[child.payload_range()];
        if &child.fourcc == b"minf" {
            rebuilt.extend_from_slice(&rebuild_minf_with_offsets(slice, span_map)?);
        } else {
            rebuilt.extend_from_slice(&build_box(&child.fourcc, slice));
        }
    }
    Ok(build_box(b"mdia", &rebuilt))
}

fn rebuild_trak_with_offsets(
    trak_payload: &[u8],
    span_map: &[(u64, u64, u64)],
) -> Result<Vec<u8>, String> {
    let mut rebuilt = Vec::with_capacity(trak_payload.len());
    for child in parse_child_boxes(trak_payload)? {
        let slice = &trak_payload[child.payload_range()];
        if &child.fourcc == b"mdia" {
            rebuilt.extend_from_slice(&rebuild_mdia_with_offsets(slice, span_map)?);
        } else {
            rebuilt.extend_from_slice(&build_box(&child.fourcc, slice));
        }
    }
    Ok(rebuilt)
}

fn mux_non_fragmented(
    video_path: &Path,
    audio_path: &Path,
    video: &Mp4Source,
    audio: &Mp4Source,
    output_path: &Path,
    cancel: &Cancellation,
    mut on_progress: impl FnMut(f32),
) -> Result<(), String> {
    if video.mdat_spans.is_empty() || audio.mdat_spans.is_empty() {
        return Err("MP4 sem caixas 'mdat' para combinar.".to_owned());
    }
    let ftyp = build_ftyp_box(video.ftyp_payload.as_deref(), audio.ftyp_payload.as_deref());
    let video_payload_total: u64 = video.mdat_spans.iter().map(|&(_, len)| len).sum();
    let audio_payload_total: u64 = audio.mdat_spans.iter().map(|&(_, len)| len).sum();
    let total_mdat_payload = video_payload_total + audio_payload_total;
    let progress_denom = total_mdat_payload.max(1);

    let out_file = File::create(output_path)
        .map_err(|e| format!("Não foi possível criar arquivo MP4 combinado: {e}"))?;
    let mut writer = BufWriter::with_capacity(COPY_CHUNK_SIZE, out_file);
    writer
        .write_all(&ftyp)
        .map_err(|e| format!("Falha ao gravar 'ftyp': {e}"))?;
    let mdat_header_len = write_box_header(&mut writer, b"mdat", total_mdat_payload)?;
    let mut cursor = ftyp.len() as u64 + mdat_header_len;

    let mut video_file = File::open(video_path)
        .map_err(|e| format!("Não foi possível abrir trilha de vídeo: {e}"))?;
    let mut audio_file = File::open(audio_path)
        .map_err(|e| format!("Não foi possível abrir trilha de áudio: {e}"))?;
    let mut copy_buf = vec![0u8; COPY_CHUNK_SIZE];
    let mut copied_bytes = 0u64;

    let mut video_span_map = Vec::with_capacity(video.mdat_spans.len());
    for &(src_off, len) in &video.mdat_spans {
        copy_exact_range(
            &mut video_file,
            src_off,
            len,
            &mut writer,
            &mut copy_buf,
            cancel,
        )?;
        video_span_map.push((src_off, len, cursor));
        cursor += len;
        copied_bytes += len;
        on_progress(((copied_bytes as f64 / progress_denom as f64) * 100.0) as f32);
    }

    let mut audio_span_map = Vec::with_capacity(audio.mdat_spans.len());
    for &(src_off, len) in &audio.mdat_spans {
        copy_exact_range(
            &mut audio_file,
            src_off,
            len,
            &mut writer,
            &mut copy_buf,
            cancel,
        )?;
        audio_span_map.push((src_off, len, cursor));
        cursor += len;
        copied_bytes += len;
        on_progress(((copied_bytes as f64 / progress_denom as f64) * 100.0) as f32);
    }

    let out_mvhd_ts = video.mvhd_timescale.max(1);
    let audio_mvhd_dur_rescaled = rescale_u64(
        audio.mvhd_duration,
        audio.mvhd_timescale.max(1),
        out_mvhd_ts,
    );
    let out_mvhd_dur = video.mvhd_duration.max(audio_mvhd_dur_rescaled);
    let mvhd_payload = patch_mvhd(&video.mvhd_payload, out_mvhd_dur, 3)?;

    let video_trak_patched = patch_trak_metadata(&video.trak_payload, 1, None)?;
    let video_trak_final = rebuild_trak_with_offsets(&video_trak_patched, &video_span_map)?;

    let audio_trak_patched = patch_trak_metadata(
        &audio.trak_payload,
        2,
        Some((audio.mvhd_timescale.max(1), out_mvhd_ts)),
    )?;
    let audio_trak_final = rebuild_trak_with_offsets(&audio_trak_patched, &audio_span_map)?;

    let mut moov_payload = Vec::new();
    moov_payload.extend_from_slice(&build_box(b"mvhd", &mvhd_payload));
    moov_payload.extend_from_slice(&build_box(b"trak", &video_trak_final));
    moov_payload.extend_from_slice(&build_box(b"trak", &audio_trak_final));
    if let Some(udta) = &video.udta_payload {
        moov_payload.extend_from_slice(&build_box(b"udta", udta));
    }
    let moov = build_box(b"moov", &moov_payload);
    writer
        .write_all(&moov)
        .map_err(|e| format!("Falha ao gravar 'moov': {e}"))?;
    writer
        .flush()
        .map_err(|e| format!("Falha ao descarregar MP4 combinado: {e}"))?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|e| format!("Falha ao sincronizar MP4 combinado: {e}"))?;
    on_progress(100.0);
    Ok(())
}

pub(crate) fn mux_mp4(
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    cancel: &Cancellation,
    on_progress: impl FnMut(f32),
) -> Result<(), String> {
    if cancel.cancelled() {
        return Err("Download cancelado.".to_owned());
    }
    let video = scan_mp4(video_path)?;
    let audio = scan_mp4(audio_path)?;

    let result = match (video.fragments.is_empty(), audio.fragments.is_empty()) {
        (false, false) => mux_fragmented(
            video_path,
            audio_path,
            &video,
            &audio,
            output_path,
            cancel,
            on_progress,
        ),
        (true, true) => mux_non_fragmented(
            video_path,
            audio_path,
            &video,
            &audio,
            output_path,
            cancel,
            on_progress,
        ),
        _ => Err(
            "Fluxos incompatíveis para muxing embutido (fragmentado vs. não fragmentado)."
                .to_owned(),
        ),
    };

    if result.is_err() {
        let _ = fs::remove_file(output_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ftyp(brands: &[[u8; 4]]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(brands.first().unwrap_or(b"isom"));
        payload.extend_from_slice(&512u32.to_be_bytes());
        for b in brands {
            payload.extend_from_slice(b);
        }
        build_box(b"ftyp", &payload)
    }

    fn make_mvhd_v0(timescale: u32, duration: u32, next_track_id: u32) -> Vec<u8> {
        let mut p = vec![0u8; 100];
        write_u32_be(&mut p, 12, timescale);
        write_u32_be(&mut p, 16, duration);
        write_u32_be(&mut p, 96, next_track_id);
        build_box(b"mvhd", &p)
    }

    fn make_tkhd_v0(track_id: u32, duration: u32) -> Vec<u8> {
        let mut p = vec![0u8; 84];
        p[3] = 3; // track_enabled | track_in_movie
        write_u32_be(&mut p, 12, track_id);
        write_u32_be(&mut p, 20, duration);
        build_box(b"tkhd", &p)
    }

    fn make_elst_v0(segment_duration: u32) -> Vec<u8> {
        let mut p = vec![0u8; 20];
        write_u32_be(&mut p, 4, 1); // entry_count = 1
        write_u32_be(&mut p, 8, segment_duration);
        build_box(b"elst", &p)
    }

    fn make_mdhd_v0(timescale: u32, duration: u32) -> Vec<u8> {
        let mut p = vec![0u8; 24];
        write_u32_be(&mut p, 12, timescale);
        write_u32_be(&mut p, 16, duration);
        build_box(b"mdhd", &p)
    }

    fn make_stco(offsets: &[u32]) -> Vec<u8> {
        let mut p = Vec::with_capacity(8 + offsets.len() * 4);
        p.extend_from_slice(&0u32.to_be_bytes());
        p.extend_from_slice(&(offsets.len() as u32).to_be_bytes());
        for &o in offsets {
            p.extend_from_slice(&o.to_be_bytes());
        }
        build_box(b"stco", &p)
    }

    fn make_minimal_trak(
        track_id: u32,
        tkhd_dur: u32,
        mdhd_ts: u32,
        elst_dur: Option<u32>,
        stco_offsets: &[u32],
    ) -> Vec<u8> {
        let mut stbl_payload = build_box(b"stsd", &[0u8; 8]);
        stbl_payload.extend_from_slice(&make_stco(stco_offsets));
        let stbl = build_box(b"stbl", &stbl_payload);
        let minf = build_box(b"minf", &stbl);
        let mut mdia_payload = make_mdhd_v0(mdhd_ts, mdhd_ts * 10);
        mdia_payload.extend_from_slice(&minf);
        let mdia = build_box(b"mdia", &mdia_payload);

        let mut trak_payload = make_tkhd_v0(track_id, tkhd_dur);
        if let Some(dur) = elst_dur {
            let edts = build_box(b"edts", &make_elst_v0(dur));
            trak_payload.extend_from_slice(&edts);
        }
        trak_payload.extend_from_slice(&mdia);
        build_box(b"trak", &trak_payload)
    }

    fn make_trex_box(track_id: u32) -> Vec<u8> {
        let mut p = vec![0u8; 24];
        write_u32_be(&mut p, 4, track_id);
        write_u32_be(&mut p, 8, 1);
        build_box(b"trex", &p)
    }

    fn make_moof(seq: u32, track_id: u32, tfdt_val: u64, base_data_offset: Option<u64>) -> Vec<u8> {
        let mut mfhd_p = vec![0u8; 8];
        write_u32_be(&mut mfhd_p, 4, seq);
        let mfhd = build_box(b"mfhd", &mfhd_p);

        let mut tfhd_p = if let Some(bdo) = base_data_offset {
            let mut buf = vec![0u8; 16];
            buf[3] = 0x01; // base-data-offset-present
            write_u32_be(&mut buf, 4, track_id);
            write_u64_be(&mut buf, 8, bdo);
            buf
        } else {
            let mut buf = vec![0u8; 8];
            buf[1] = 0x02; // default-base-is-moof
            write_u32_be(&mut buf, 4, track_id);
            buf
        };
        let _ = &mut tfhd_p;
        let tfhd = build_box(b"tfhd", &tfhd_p);

        let mut tfdt_p = vec![0u8; 12];
        tfdt_p[0] = 1;
        write_u64_be(&mut tfdt_p, 4, tfdt_val);
        let tfdt = build_box(b"tfdt", &tfdt_p);

        let mut traf_p = Vec::new();
        traf_p.extend_from_slice(&tfhd);
        traf_p.extend_from_slice(&tfdt);
        let traf = build_box(b"traf", &traf_p);

        let mut moof_p = Vec::new();
        moof_p.extend_from_slice(&mfhd);
        moof_p.extend_from_slice(&traf);
        build_box(b"moof", &moof_p)
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fasttube-mux-unit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn muxes_fragmented_mp4_interleaving_timestamps_and_writing_mfra() {
        let dir = temp_dir("fmp4");
        let v_path = dir.join("video.mp4");
        let a_path = dir.join("audio.m4a");
        let out_path = dir.join("merged.mp4");

        // Video: mvhd ts=1000, dur=5000, mdhd ts=30000
        let mut v_bytes = Vec::new();
        v_bytes.extend_from_slice(&make_ftyp(&[*b"iso6", *b"dash"]));
        let mut v_moov = make_mvhd_v0(1000, 5000, 2);
        v_moov.extend_from_slice(&make_minimal_trak(1, 5000, 30000, None, &[]));
        let v_mvex = build_box(b"mvex", &make_trex_box(1));
        v_moov.extend_from_slice(&v_mvex);
        v_bytes.extend_from_slice(&build_box(b"moov", &v_moov));
        // Fragment 1 at t=0s (tfdt=0), Fragment 2 at t=2s (tfdt=60000) with base_data_offset
        let v_moof1 = make_moof(1, 1, 0, None);
        let v_mdat1 = build_box(b"mdat", b"VID1");
        v_bytes.extend_from_slice(&v_moof1);
        v_bytes.extend_from_slice(&v_mdat1);
        let v_moof2_off = v_bytes.len() as u64;
        let v_moof2 = make_moof(2, 1, 60000, Some(v_moof2_off + 100));
        let v_mdat2 = build_box(b"mdat", b"VID2");
        v_bytes.extend_from_slice(&v_moof2);
        v_bytes.extend_from_slice(&v_mdat2);
        fs::write(&v_path, &v_bytes).unwrap();

        // Audio: mvhd ts=44100, dur=264600 (6s), mdhd ts=44100, elst dur=264600
        let mut a_bytes = Vec::new();
        a_bytes.extend_from_slice(&make_ftyp(&[*b"M4A ", *b"mp42"]));
        let mut a_moov = make_mvhd_v0(44100, 264600, 2);
        a_moov.extend_from_slice(&make_minimal_trak(1, 264600, 44100, Some(264600), &[]));
        let a_mvex = build_box(b"mvex", &make_trex_box(1));
        a_moov.extend_from_slice(&a_mvex);
        a_bytes.extend_from_slice(&build_box(b"moov", &a_moov));
        // Audio Fragment 1 at t=1s (tfdt=44100), Fragment 2 at t=3s (tfdt=132300)
        a_bytes.extend_from_slice(&make_moof(1, 1, 44100, None));
        a_bytes.extend_from_slice(&build_box(b"mdat", b"AUD1"));
        a_bytes.extend_from_slice(&make_moof(2, 1, 132300, None));
        a_bytes.extend_from_slice(&build_box(b"mdat", b"AUD2"));
        fs::write(&a_path, &a_bytes).unwrap();

        let cancel = Cancellation::default();
        let mut seen_progress = Vec::new();
        mux_mp4(&v_path, &a_path, &out_path, &cancel, |p| {
            seen_progress.push(p);
        })
        .unwrap();
        assert_eq!(seen_progress.last().copied(), Some(100.0));

        let merged = fs::read(&out_path).unwrap();
        let top = parse_child_boxes(&merged).unwrap();
        let tags: Vec<[u8; 4]> = top.iter().map(|b| b.fourcc).collect();
        assert_eq!(
            tags,
            vec![
                *b"ftyp", *b"moov", *b"moof", *b"mdat", *b"moof", *b"mdat", *b"moof", *b"mdat",
                *b"moof", *b"mdat", *b"mfra"
            ]
        );

        // Verify ftyp contains brands from both inputs
        let ftyp_payload = &merged[top[0].payload_range()];
        assert!(ftyp_payload.windows(4).any(|w| w == b"dash"));
        assert!(ftyp_payload.windows(4).any(|w| w == b"M4A "));

        // Verify moov: mvhd next_track_ID=3 and rescaled duration=6000; audio tkhd/elst rescaled=6000
        let moov_payload = &merged[top[1].payload_range()];
        let moov_children = parse_child_boxes(moov_payload).unwrap();
        let mvhd_p = &moov_payload[moov_children[0].payload_range()];
        assert_eq!(read_u32_be(mvhd_p, 12), Some(1000));
        assert_eq!(read_u32_be(mvhd_p, 16), Some(6000));
        assert_eq!(read_u32_be(mvhd_p, 96), Some(3));

        let a_trak_p = &moov_payload[moov_children[2].payload_range()];
        let a_trak_children = parse_child_boxes(a_trak_p).unwrap();
        let a_tkhd_p = &a_trak_p[a_trak_children[0].payload_range()];
        assert_eq!(read_u32_be(a_tkhd_p, 12), Some(2));
        assert_eq!(read_u32_be(a_tkhd_p, 20), Some(6000));
        let a_edts_p = &a_trak_p[a_trak_children[1].payload_range()];
        let edts_children = parse_child_boxes(a_edts_p).unwrap();
        let a_elst_p = &a_edts_p[edts_children[0].payload_range()];
        assert_eq!(read_u32_be(a_elst_p, 8), Some(6000));

        // Verify mvex has trex track_ID 1 and 2
        let mvex_p = &moov_payload[moov_children[3].payload_range()];
        let mvex_children = parse_child_boxes(mvex_p).unwrap();
        assert_eq!(
            read_u32_be(&mvex_p[mvex_children[0].payload_range()], 4),
            Some(1)
        );
        assert_eq!(
            read_u32_be(&mvex_p[mvex_children[1].payload_range()], 4),
            Some(2)
        );

        // Verify chronological interleaving: VID1 (0s), AUD1 (1s), VID2 (2s), AUD2 (3s)
        assert_eq!(&merged[top[3].payload_range()], b"VID1");
        assert_eq!(&merged[top[5].payload_range()], b"AUD1");
        assert_eq!(&merged[top[7].payload_range()], b"VID2");
        assert_eq!(&merged[top[9].payload_range()], b"AUD2");

        // Verify moof #3 (VID2) had sequence_number=3, track_ID=1, and shifted base_data_offset
        let moof3_bytes = &merged[top[6].offset..top[6].offset + top[6].size];
        let moof3_p = &merged[top[6].payload_range()];
        let moof3_ch = parse_child_boxes(moof3_p).unwrap();
        assert_eq!(
            read_u32_be(&moof3_p[moof3_ch[0].payload_range()], 4),
            Some(3)
        );
        let traf_p = &moof3_p[moof3_ch[1].payload_range()];
        let traf_ch = parse_child_boxes(traf_p).unwrap();
        let tfhd_p = &traf_p[traf_ch[0].payload_range()];
        assert_eq!(read_u32_be(tfhd_p, 4), Some(1));
        assert_eq!(read_u64_be(tfhd_p, 8), Some(top[6].offset as u64 + 100));
        let _ = moof3_bytes;

        // Verify mfra/mfro total size and tfra entries point to exact moof offsets
        let mfra_span = top.last().unwrap();
        let mfra_p = &merged[mfra_span.payload_range()];
        let mfra_ch = parse_child_boxes(mfra_p).unwrap();
        assert_eq!(mfra_ch.len(), 3);
        let v_tfra_p = &mfra_p[mfra_ch[0].payload_range()];
        assert_eq!(read_u32_be(v_tfra_p, 4), Some(1));
        assert_eq!(read_u32_be(v_tfra_p, 12), Some(2));
        assert_eq!(read_u64_be(v_tfra_p, 16), Some(0));
        assert_eq!(read_u64_be(v_tfra_p, 24), Some(top[2].offset as u64));
        assert_eq!(read_u64_be(v_tfra_p, 16 + 19), Some(60000));
        assert_eq!(read_u64_be(v_tfra_p, 24 + 19), Some(top[6].offset as u64));
        let mfro_p = &mfra_p[mfra_ch[2].payload_range()];
        assert_eq!(read_u32_be(mfro_p, 4), Some(mfra_span.size as u32));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn muxes_non_fragmented_mp4_updating_stco_offsets() {
        let dir = temp_dir("nonfrag");
        let v_path = dir.join("video.mp4");
        let a_path = dir.join("audio.m4a");
        let out_path = dir.join("merged.mp4");

        // Construct video with ftyp + moov + mdat ("VVVV")
        let v_ftyp = make_ftyp(&[*b"isom"]);
        let v_moov_placeholder = build_box(
            b"moov",
            &[
                make_mvhd_v0(1000, 2000, 2),
                make_minimal_trak(1, 2000, 30000, None, &[0]),
            ]
            .concat(),
        );
        let v_mdat_payload_start = (v_ftyp.len() + v_moov_placeholder.len() + 8) as u32;
        let v_moov = build_box(
            b"moov",
            &[
                make_mvhd_v0(1000, 2000, 2),
                make_minimal_trak(1, 2000, 30000, None, &[v_mdat_payload_start]),
            ]
            .concat(),
        );
        let mut v_bytes = Vec::new();
        v_bytes.extend_from_slice(&v_ftyp);
        v_bytes.extend_from_slice(&v_moov);
        v_bytes.extend_from_slice(&build_box(b"mdat", b"VVVV"));
        fs::write(&v_path, &v_bytes).unwrap();

        // Construct audio with ftyp + moov + mdat ("AAAA")
        let a_ftyp = make_ftyp(&[*b"M4A "]);
        let a_moov_placeholder = build_box(
            b"moov",
            &[
                make_mvhd_v0(44100, 88200, 2),
                make_minimal_trak(1, 88200, 44100, None, &[0]),
            ]
            .concat(),
        );
        let a_mdat_payload_start = (a_ftyp.len() + a_moov_placeholder.len() + 8) as u32;
        let a_moov = build_box(
            b"moov",
            &[
                make_mvhd_v0(44100, 88200, 2),
                make_minimal_trak(1, 88200, 44100, None, &[a_mdat_payload_start]),
            ]
            .concat(),
        );
        let mut a_bytes = Vec::new();
        a_bytes.extend_from_slice(&a_ftyp);
        a_bytes.extend_from_slice(&a_moov);
        a_bytes.extend_from_slice(&build_box(b"mdat", b"AAAA"));
        fs::write(&a_path, &a_bytes).unwrap();

        mux_mp4(
            &v_path,
            &a_path,
            &out_path,
            &Cancellation::default(),
            |_| {},
        )
        .unwrap();

        let merged = fs::read(&out_path).unwrap();
        let top = parse_child_boxes(&merged).unwrap();
        let tags: Vec<[u8; 4]> = top.iter().map(|b| b.fourcc).collect();
        assert_eq!(tags, vec![*b"ftyp", *b"mdat", *b"moov"]);
        assert_eq!(&merged[top[1].payload_range()], b"VVVVAAAA");

        let moov_p = &merged[top[2].payload_range()];
        let moov_ch = parse_child_boxes(moov_p).unwrap();

        // Check video stco offset points to "VVVV"
        let v_trak_p = &moov_p[moov_ch[1].payload_range()];
        let v_mdia_p = &v_trak_p[parse_child_boxes(v_trak_p).unwrap()[1].payload_range()];
        let v_minf_p = &v_mdia_p[parse_child_boxes(v_mdia_p).unwrap()[1].payload_range()];
        let v_stbl_p = &v_minf_p[parse_child_boxes(v_minf_p).unwrap()[0].payload_range()];
        let v_stco_p = &v_stbl_p[parse_child_boxes(v_stbl_p).unwrap()[1].payload_range()];
        let v_off = read_u32_be(v_stco_p, 8).unwrap() as usize;
        assert_eq!(&merged[v_off..v_off + 4], b"VVVV");

        // Check audio stco offset points to "AAAA"
        let a_trak_p = &moov_p[moov_ch[2].payload_range()];
        let a_mdia_p = &a_trak_p[parse_child_boxes(a_trak_p).unwrap()[1].payload_range()];
        let a_minf_p = &a_mdia_p[parse_child_boxes(a_mdia_p).unwrap()[1].payload_range()];
        let a_stbl_p = &a_minf_p[parse_child_boxes(a_minf_p).unwrap()[0].payload_range()];
        let a_stco_p = &a_stbl_p[parse_child_boxes(a_stbl_p).unwrap()[1].payload_range()];
        let a_off = read_u32_be(a_stco_p, 8).unwrap() as usize;
        assert_eq!(&merged[a_off..a_off + 4], b"AAAA");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancels_muxing_cleanly_and_rejects_invalid_mp4() {
        let dir = temp_dir("cancel");
        let v_path = dir.join("v.mp4");
        let a_path = dir.join("a.m4a");
        let out_path = dir.join("out.mp4");

        fs::write(&v_path, b"not an mp4").unwrap();
        fs::write(&a_path, b"not an mp4").unwrap();

        let err = mux_mp4(
            &v_path,
            &a_path,
            &out_path,
            &Cancellation::default(),
            |_| {},
        )
        .unwrap_err();
        assert!(!err.is_empty());
        assert!(!out_path.exists());

        let cancel = Cancellation::default();
        cancel.cancel();
        let err = mux_mp4(&v_path, &a_path, &out_path, &cancel, |_| {}).unwrap_err();
        assert!(err.contains("cancelado"));

        let _ = fs::remove_dir_all(&dir);
    }
}
