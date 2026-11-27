//! The clip file: a plain MP4 (ISO base media, not fragmented, `moov` before `mdat` so a reader can start at once) with
//! one H.264 video track, one Opus audio track per source, and the clip metadata in a `uuid` box at the very end.
//!
//! The official clip's box layout is not in the bundle (the native module writes it), so these choices are ours: Opus in MP4 (the "Opus" sample entry with its `dOps` box, as the Opus-in-ISOBMFF
//! specification defines it), because the stream's own audio is Opus and no AAC encoder is in the tree; and the
//! metadata in a top-level `uuid` box last in the file, because the official upload path looks for the final `uuid`
//! box (module 489496 `VO`, "mp4boxInputFile" loop) and keeps its first 24 bytes.
use crate::sps::SpsInfo;
use std::io::{self, Read, Seek, SeekFrom, Write};

pub const VIDEO_TIMESCALE: u32 = 90_000;
pub const AUDIO_TIMESCALE: u32 = 48_000;
/// Samples in one Opus packet, which is also its duration in `AUDIO_TIMESCALE` ticks.
pub const AUDIO_PACKET_TICKS: u32 = 960;
/// Official client clip metadata box UUID (`a1c85299-3346-4db8-88f0-83f57a75a5ef`).
pub const METADATA_UUID: [u8; 16] = [
    0xa1, 0xc8, 0x52, 0x99, 0x33, 0x46, 0x4d, 0xb8, 0x88, 0xf0, 0x83, 0xf5, 0x7a, 0x75, 0xa5,
    0xef,
];
/// Legacy clip metadata box UUID written by earlier versions; still accepted on read.
pub const LEGACY_METADATA_UUID: [u8; 16] = *b"gravecord/clip06";

fn is_metadata_uuid(user: &[u8; 16]) -> bool {
    *user == METADATA_UUID || *user == LEGACY_METADATA_UUID
}
/// Seconds' worth of samples per chunk, so a reader seeking in the file reads one second at a time.
const CHUNK_TICKS: u64 = VIDEO_TIMESCALE as u64;
const CHUNK_PACKETS: usize = 50;

pub struct VideoSample {
    /// Length-prefixed NAL units (four byte lengths), without parameter sets and access unit delimiters.
    pub data: Vec<u8>,
    /// Duration in `VIDEO_TIMESCALE` ticks.
    pub duration: u32,
    pub sync: bool,
}

pub struct VideoInput {
    pub sps: Vec<u8>,
    pub pps: Vec<u8>,
    pub info: SpsInfo,
    pub samples: Vec<VideoSample>,
}

pub struct AudioInput {
    /// The track's name (the `hdlr` name), such as "application" or "voice".
    pub name: String,
    pub channels: u8,
    pub pre_skip: u16,
    /// Opus packets of 960 samples each.
    pub packets: Vec<Vec<u8>>,
}

/// Annex B to length-prefixed NAL units, leaving out the SPS, PPS and access unit delimiters (the parameter sets live in
/// the sample entry).
pub fn to_length_prefixed(annex_b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(annex_b.len());
    for nal in dc_media::h264::nals(annex_b) {
        if matches!(nal[0] & 31, 7..=9) {
            continue;
        }
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

// ----- writing -----

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut inner = vec![version];
    inner.extend_from_slice(&flags.to_be_bytes()[1..]);
    inner.extend_from_slice(body);
    bx(kind, &inner)
}

trait Put {
    fn u16(&mut self, value: u16) -> &mut Self;
    fn u32(&mut self, value: u32) -> &mut Self;
    fn zeros(&mut self, count: usize) -> &mut Self;
}

impl Put for Vec<u8> {
    fn u16(&mut self, value: u16) -> &mut Self {
        self.extend_from_slice(&value.to_be_bytes());
        self
    }
    fn u32(&mut self, value: u32) -> &mut Self {
        self.extend_from_slice(&value.to_be_bytes());
        self
    }
    fn zeros(&mut self, count: usize) -> &mut Self {
        self.resize(self.len() + count, 0);
        self
    }
}

const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

/// A track's samples as the tables want them.
struct Table {
    sizes: Vec<u32>,
    /// `(sample count, delta)` runs.
    deltas: Vec<(u32, u32)>,
    sync: Option<Vec<u32>>,
    /// Samples in each chunk.
    chunks: Vec<u32>,
    /// Start of each chunk in milliseconds, for the interleave.
    chunk_start_ms: Vec<u64>,
    /// Media ticks.
    duration: u64,
    timescale: u32,
}

fn video_table(video: &VideoInput) -> Table {
    let mut table = Table {
        sizes: Vec::new(),
        deltas: Vec::new(),
        sync: Some(Vec::new()),
        chunks: Vec::new(),
        chunk_start_ms: Vec::new(),
        duration: 0,
        timescale: VIDEO_TIMESCALE,
    };
    let mut in_chunk = 0u32;
    let mut chunk_ticks = 0u64;
    for (index, sample) in video.samples.iter().enumerate() {
        if in_chunk == 0 {
            table
                .chunk_start_ms
                .push(table.duration * 1000 / u64::from(VIDEO_TIMESCALE));
        }
        table.sizes.push(sample.data.len() as u32);
        match table.deltas.last_mut() {
            Some((count, delta)) if *delta == sample.duration => *count += 1,
            _ => table.deltas.push((1, sample.duration)),
        }
        if sample.sync
            && let Some(sync) = table.sync.as_mut()
        {
            sync.push(index as u32 + 1);
        }
        table.duration += u64::from(sample.duration);
        chunk_ticks += u64::from(sample.duration);
        in_chunk += 1;
        if chunk_ticks >= CHUNK_TICKS {
            table.chunks.push(in_chunk);
            in_chunk = 0;
            chunk_ticks = 0;
        }
    }
    if in_chunk > 0 {
        table.chunks.push(in_chunk);
    }
    table
}

fn audio_table(audio: &AudioInput) -> Table {
    let count = audio.packets.len();
    let mut chunks = Vec::new();
    let mut chunk_start_ms = Vec::new();
    for (index, _) in audio.packets.chunks(CHUNK_PACKETS).enumerate() {
        let first = index * CHUNK_PACKETS;
        chunk_start_ms
            .push(first as u64 * u64::from(AUDIO_PACKET_TICKS) * 1000 / u64::from(AUDIO_TIMESCALE));
        chunks.push((count - first).min(CHUNK_PACKETS) as u32);
    }
    Table {
        sizes: audio
            .packets
            .iter()
            .map(|packet| packet.len() as u32)
            .collect(),
        deltas: if count == 0 {
            Vec::new()
        } else {
            vec![(count as u32, AUDIO_PACKET_TICKS)]
        },
        sync: None,
        chunks,
        chunk_start_ms,
        duration: count as u64 * u64::from(AUDIO_PACKET_TICKS),
        timescale: AUDIO_TIMESCALE,
    }
}

fn sample_tables(table: &Table, entry: Vec<u8>, offsets: &[u32]) -> Vec<u8> {
    let mut stsd = Vec::new();
    stsd.u32(1).extend_from_slice(&entry);
    let mut stts = Vec::new();
    stts.u32(table.deltas.len() as u32);
    for (count, delta) in &table.deltas {
        stts.u32(*count).u32(*delta);
    }
    let mut stsc = Vec::new();
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for (index, samples) in table.chunks.iter().enumerate() {
        if runs.last().is_none_or(|(_, last)| last != samples) {
            runs.push((index as u32 + 1, *samples));
        }
    }
    stsc.u32(runs.len() as u32);
    for (first, samples) in runs {
        stsc.u32(first).u32(samples).u32(1);
    }
    let mut stsz = Vec::new();
    stsz.u32(0).u32(table.sizes.len() as u32);
    for size in &table.sizes {
        stsz.u32(*size);
    }
    let mut stco = Vec::new();
    stco.u32(offsets.len() as u32);
    for offset in offsets {
        stco.u32(*offset);
    }
    let mut stbl = full(b"stsd", 0, 0, &stsd);
    stbl.extend(full(b"stts", 0, 0, &stts));
    if let Some(sync) = &table.sync {
        let mut stss = Vec::new();
        stss.u32(sync.len() as u32);
        for number in sync {
            stss.u32(*number);
        }
        stbl.extend(full(b"stss", 0, 0, &stss));
    }
    stbl.extend(full(b"stsc", 0, 0, &stsc));
    stbl.extend(full(b"stsz", 0, 0, &stsz));
    stbl.extend(full(b"stco", 0, 0, &stco));
    bx(b"stbl", &stbl)
}

fn avc1_entry(video: &VideoInput) -> Vec<u8> {
    let info = &video.info;
    let mut avcc = vec![1, info.profile, info.compatibility, info.level, 0xFF, 0xE1];
    avcc.u16(video.sps.len() as u16)
        .extend_from_slice(&video.sps);
    avcc.push(1);
    avcc.u16(video.pps.len() as u16)
        .extend_from_slice(&video.pps);
    if matches!(info.profile, 100 | 110 | 122 | 144) {
        avcc.push(0xFC | info.chroma_format);
        avcc.push(0xF8 | (info.bit_depth_luma - 8));
        avcc.push(0xF8 | (info.bit_depth_chroma - 8));
        avcc.push(0);
    }
    let mut entry = Vec::new();
    entry.zeros(6).u16(1).zeros(16);
    entry.u16(info.width as u16).u16(info.height as u16);
    entry.u32(0x0048_0000).u32(0x0048_0000).zeros(4).u16(1);
    entry.zeros(32).u16(0x0018).u16(0xFFFF);
    entry.extend(bx(b"avcC", &avcc));
    bx(b"avc1", &entry)
}

fn opus_entry(audio: &AudioInput) -> Vec<u8> {
    let mut dops = vec![0, audio.channels];
    dops.u16(audio.pre_skip).u32(AUDIO_TIMESCALE).u16(0);
    dops.push(0);
    let mut entry = Vec::new();
    entry.zeros(6).u16(1).zeros(8);
    entry
        .u16(u16::from(audio.channels))
        .u16(16)
        .u16(0)
        .u16(0)
        .u32(AUDIO_TIMESCALE << 16);
    entry.extend(bx(b"dOps", &dops));
    bx(b"Opus", &entry)
}

struct TrackPlan<'a> {
    id: u32,
    handler: &'static [u8; 4],
    name: &'a str,
    table: Table,
    entry: Vec<u8>,
    width: u32,
    height: u32,
}

fn build_moov(plans: &[TrackPlan], offsets: &[Vec<u32>]) -> Vec<u8> {
    let movie_ms = plans
        .iter()
        .map(|plan| plan.table.duration * 1000 / u64::from(plan.table.timescale))
        .max()
        .unwrap_or(0);
    let mut mvhd = Vec::new();
    mvhd.u32(0)
        .u32(0)
        .u32(1000)
        .u32(movie_ms as u32)
        .u32(0x0001_0000)
        .u16(0x0100)
        .zeros(10);
    for value in MATRIX {
        mvhd.u32(value);
    }
    mvhd.zeros(24).u32(plans.len() as u32 + 1);
    let mut moov = full(b"mvhd", 0, 0, &mvhd);
    for (plan, offsets) in plans.iter().zip(offsets) {
        let video = plan.handler == b"vide";
        let track_ms = plan.table.duration * 1000 / u64::from(plan.table.timescale);
        let mut tkhd = Vec::new();
        tkhd.u32(0)
            .u32(0)
            .u32(plan.id)
            .u32(0)
            .u32(track_ms as u32)
            .zeros(8);
        tkhd.u16(0)
            .u16(if video { 0 } else { 1 })
            .u16(if video { 0 } else { 0x0100 })
            .u16(0);
        for value in MATRIX {
            tkhd.u32(value);
        }
        tkhd.u32(plan.width << 16).u32(plan.height << 16);
        let mut mdhd = Vec::new();
        mdhd.u32(0)
            .u32(0)
            .u32(plan.table.timescale)
            .u32(plan.table.duration as u32)
            .u16(0x55C4)
            .u16(0);
        let mut hdlr = Vec::new();
        hdlr.u32(0).extend_from_slice(plan.handler);
        hdlr.zeros(12);
        hdlr.extend_from_slice(plan.name.as_bytes());
        hdlr.push(0);
        let mut header = Vec::new();
        let media_header = if video {
            header.u16(0).zeros(6);
            full(b"vmhd", 0, 1, &header)
        } else {
            header.u16(0).u16(0);
            full(b"smhd", 0, 0, &header)
        };
        let url = full(b"url ", 0, 1, &[]);
        let mut dref = Vec::new();
        dref.u32(1).extend_from_slice(&url);
        let dinf = bx(b"dinf", &full(b"dref", 0, 0, &dref));
        let mut minf = media_header;
        minf.extend(dinf);
        minf.extend(sample_tables(&plan.table, plan.entry.clone(), offsets));
        let mut mdia = full(b"mdhd", 0, 0, &mdhd);
        mdia.extend(full(b"hdlr", 0, 0, &hdlr));
        mdia.extend(bx(b"minf", &minf));
        let mut trak = full(b"tkhd", 0, 3, &tkhd);
        trak.extend(bx(b"mdia", &mdia));
        moov.extend(bx(b"trak", &trak));
    }
    bx(b"moov", &moov)
}

/// Writes the clip. Returns the file's size in bytes. The video is the first track, then one track per audio input.
pub fn write(
    out: &mut impl Write,
    video: &VideoInput,
    audio: &[AudioInput],
    metadata: &[u8],
) -> io::Result<u64> {
    let mut plans = vec![TrackPlan {
        id: 1,
        handler: b"vide",
        name: "video",
        table: video_table(video),
        entry: avc1_entry(video),
        width: video.info.width,
        height: video.info.height,
    }];
    for (index, track) in audio.iter().enumerate() {
        plans.push(TrackPlan {
            id: index as u32 + 2,
            handler: b"soun",
            name: &track.name,
            table: audio_table(track),
            entry: opus_entry(track),
            width: 0,
            height: 0,
        });
    }
    // The order of the chunks in `mdat`: by start time, the video first on a tie.
    let mut order: Vec<(u64, usize, usize)> = Vec::new();
    for (track, plan) in plans.iter().enumerate() {
        for (chunk, start) in plan.table.chunk_start_ms.iter().enumerate() {
            order.push((*start, track, chunk));
        }
    }
    order.sort_unstable();
    let ftyp = {
        let mut body = Vec::new();
        body.extend_from_slice(b"isom");
        body.u32(512);
        for brand in [b"isom", b"iso2", b"avc1", b"mp41"] {
            body.extend_from_slice(brand);
        }
        bx(b"ftyp", &body)
    };
    let zero: Vec<Vec<u32>> = plans
        .iter()
        .map(|plan| vec![0; plan.table.chunks.len()])
        .collect();
    let moov_size = build_moov(&plans, &zero).len() as u64;
    let mdat_start = ftyp.len() as u64 + moov_size + 8;
    // Chunk byte lengths, then offsets in the interleaved order.
    let chunk_bytes = |track: usize, chunk: usize| -> u64 {
        let plan = &plans[track];
        let first: usize = plan.table.chunks[..chunk].iter().map(|n| *n as usize).sum();
        let count = plan.table.chunks[chunk] as usize;
        plan.table.sizes[first..first + count]
            .iter()
            .map(|s| u64::from(*s))
            .sum()
    };
    let mut offsets: Vec<Vec<u32>> = zero;
    let mut cursor = mdat_start;
    for (_, track, chunk) in &order {
        if cursor > u64::from(u32::MAX) {
            return Err(io::Error::other("clip larger than 4 GiB"));
        }
        offsets[*track][*chunk] = cursor as u32;
        cursor += chunk_bytes(*track, *chunk);
    }
    let mdat_size = cursor - mdat_start + 8;
    if mdat_size > u64::from(u32::MAX) {
        return Err(io::Error::other("clip larger than 4 GiB"));
    }
    let moov = build_moov(&plans, &offsets);
    debug_assert_eq!(moov.len() as u64, moov_size);
    out.write_all(&ftyp)?;
    out.write_all(&moov)?;
    out.write_all(&(mdat_size as u32).to_be_bytes())?;
    out.write_all(b"mdat")?;
    let mut video_at = 0usize;
    let mut audio_at = vec![0usize; audio.len()];
    let mut next_chunk = vec![0usize; plans.len()];
    for (_, track, chunk) in &order {
        debug_assert_eq!(next_chunk[*track], *chunk);
        next_chunk[*track] += 1;
        let count = plans[*track].table.chunks[*chunk] as usize;
        if *track == 0 {
            for sample in &video.samples[video_at..video_at + count] {
                out.write_all(&sample.data)?;
            }
            video_at += count;
        } else {
            let at = &mut audio_at[*track - 1];
            for packet in &audio[*track - 1].packets[*at..*at + count] {
                out.write_all(packet)?;
            }
            *at += count;
        }
    }
    let mut payload = METADATA_UUID.to_vec();
    payload.extend_from_slice(metadata);
    out.write_all(&bx(b"uuid", &payload))?;
    Ok(cursor + 8 + payload.len() as u64)
}

// ----- reading -----

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub sync_samples: u32,
    pub duration_ms: u64,
    /// The sample entry's four characters, `avc1` for these files.
    pub codec: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioInfo {
    pub name: String,
    pub codec: [u8; 4],
    pub channels: u16,
    pub samples: u32,
    pub duration_ms: u64,
}

/// What a clip file says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub size: u64,
    pub duration_ms: u64,
    pub video: Option<VideoInfo>,
    pub audio: Vec<AudioInfo>,
    /// The metadata document (the payload of the `uuid` box), if the file has one.
    pub metadata: Option<Vec<u8>>,
}

fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

/// The boxes of a buffer, as `(type, body)`.
fn children(mut data: &[u8]) -> io::Result<Vec<([u8; 4], &[u8])>> {
    let mut out = Vec::new();
    while !data.is_empty() {
        if data.len() < 8 {
            return Err(bad("truncated box header"));
        }
        let size = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = data[4..8].try_into().unwrap();
        let size = if size == 0 { data.len() } else { size };
        if size < 8 || size > data.len() {
            return Err(bad("box size out of range"));
        }
        out.push((kind, &data[8..size]));
        data = &data[size..];
    }
    Ok(out)
}

fn find<'a>(boxes: &[([u8; 4], &'a [u8])], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes.iter().find(|(k, _)| k == kind).map(|(_, body)| *body)
}

fn be32(data: &[u8], at: usize) -> io::Result<u32> {
    data.get(at..at + 4)
        .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
        .ok_or_else(|| bad("short box"))
}

fn parse_track(body: &[u8]) -> io::Result<(Option<VideoInfo>, Option<AudioInfo>)> {
    let trak = children(body)?;
    let tkhd = find(&trak, b"tkhd").ok_or_else(|| bad("track without tkhd"))?;
    let tkhd_end = tkhd.len();
    let width = be32(tkhd, tkhd_end.saturating_sub(8))? >> 16;
    let height = be32(tkhd, tkhd_end.saturating_sub(4))? >> 16;
    let mdia = children(find(&trak, b"mdia").ok_or_else(|| bad("track without mdia"))?)?;
    let mdhd = find(&mdia, b"mdhd").ok_or_else(|| bad("track without mdhd"))?;
    let (timescale, duration) = (be32(mdhd, 12)?, be32(mdhd, 16)?);
    let duration_ms = u64::from(duration) * 1000 / u64::from(timescale.max(1));
    let hdlr = find(&mdia, b"hdlr").ok_or_else(|| bad("track without hdlr"))?;
    let handler: [u8; 4] = hdlr
        .get(8..12)
        .ok_or_else(|| bad("short hdlr"))?
        .try_into()
        .unwrap();
    let name_bytes = hdlr.get(24..).unwrap_or(&[]);
    let name =
        String::from_utf8_lossy(name_bytes.split(|b| *b == 0).next().unwrap_or(&[])).into_owned();
    let minf = children(find(&mdia, b"minf").ok_or_else(|| bad("track without minf"))?)?;
    let stbl = children(find(&minf, b"stbl").ok_or_else(|| bad("track without stbl"))?)?;
    let stsd = find(&stbl, b"stsd").ok_or_else(|| bad("track without stsd"))?;
    let entry = stsd.get(8..).ok_or_else(|| bad("short stsd"))?;
    let entry_type: [u8; 4] = entry
        .get(4..8)
        .ok_or_else(|| bad("short sample entry"))?
        .try_into()
        .unwrap();
    let samples = be32(
        find(&stbl, b"stsz").ok_or_else(|| bad("track without stsz"))?,
        8,
    )?;
    match &handler {
        b"vide" => {
            let sync_samples = match find(&stbl, b"stss") {
                Some(stss) => be32(stss, 4)?,
                None => samples,
            };
            Ok((
                Some(VideoInfo {
                    width,
                    height,
                    samples,
                    sync_samples,
                    duration_ms,
                    codec: entry_type,
                }),
                None,
            ))
        }
        b"soun" => {
            let channels = entry
                .get(24..26)
                .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
                .ok_or_else(|| bad("short audio entry"))?;
            Ok((
                None,
                Some(AudioInfo {
                    name,
                    codec: entry_type,
                    channels,
                    samples,
                    duration_ms,
                }),
            ))
        }
        _ => Ok((None, None)),
    }
}

/// Reads the structure of a clip file and its metadata box. Only `moov` and the metadata box are read into memory.
pub fn read_info(file: &mut (impl Read + Seek)) -> io::Result<FileInfo> {
    let size = file.seek(SeekFrom::End(0))?;
    let mut at = 0u64;
    let mut info = FileInfo {
        size,
        duration_ms: 0,
        video: None,
        audio: Vec::new(),
        metadata: None,
    };
    let mut saw_moov = false;
    while at < size {
        file.seek(SeekFrom::Start(at))?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let mut length = u64::from(u32::from_be_bytes(header[..4].try_into().unwrap()));
        let kind: [u8; 4] = header[4..8].try_into().unwrap();
        let mut head = 8u64;
        if length == 1 {
            let mut big = [0u8; 8];
            file.read_exact(&mut big)?;
            length = u64::from_be_bytes(big);
            head = 16;
        } else if length == 0 {
            length = size - at;
        }
        if length < head || at + length > size {
            return Err(bad("top-level box size out of range"));
        }
        let body_len = (length - head) as usize;
        match &kind {
            b"moov" => {
                let mut body = vec![0u8; body_len];
                file.read_exact(&mut body)?;
                saw_moov = true;
                let boxes = children(&body)?;
                let mvhd = find(&boxes, b"mvhd").ok_or_else(|| bad("moov without mvhd"))?;
                info.duration_ms =
                    u64::from(be32(mvhd, 16)?) * 1000 / u64::from(be32(mvhd, 12)?.max(1));
                for (kind, track) in &boxes {
                    if kind == b"trak" {
                        let (video, audio) = parse_track(track)?;
                        info.video = info.video.take().or(video);
                        info.audio.extend(audio);
                    }
                }
            }
            b"uuid" if body_len >= 16 => {
                let mut user = [0u8; 16];
                file.read_exact(&mut user)?;
                if is_metadata_uuid(&user) {
                    let mut payload = vec![0u8; body_len - 16];
                    file.read_exact(&mut payload)?;
                    info.metadata = Some(payload);
                }
            }
            _ => {}
        }
        at += length;
    }
    if !saw_moov {
        return Err(bad("no moov box"));
    }
    Ok(info)
}

/// Where each sample of a track lies in the file: `(offset, size)` in sample order, from `stsz`, `stsc` and `stco`.
fn sample_locations(stbl: &[([u8; 4], &[u8])]) -> io::Result<Vec<(u64, u32)>> {
    let stsz = find(stbl, b"stsz").ok_or_else(|| bad("track without stsz"))?;
    let stsc = find(stbl, b"stsc").ok_or_else(|| bad("track without stsc"))?;
    let stco = find(stbl, b"stco").ok_or_else(|| bad("track without stco"))?;
    let count = be32(stsz, 8)? as usize;
    let mut sizes = Vec::with_capacity(count);
    for index in 0..count {
        sizes.push(be32(stsz, 12 + index * 4)?);
    }
    let runs = be32(stsc, 4)? as usize;
    let mut table = Vec::with_capacity(runs);
    for index in 0..runs {
        table.push((
            be32(stsc, 8 + index * 12)? as usize,
            be32(stsc, 12 + index * 12)? as usize,
        ));
    }
    let chunks = be32(stco, 4)? as usize;
    let mut out = Vec::with_capacity(count);
    let mut sample = 0;
    for chunk in 0..chunks {
        let per_chunk = table
            .iter()
            .rev()
            .find(|(first, _)| *first <= chunk + 1)
            .map_or(0, |(_, per)| *per);
        let mut at = u64::from(be32(stco, 8 + chunk * 4)?);
        for _ in 0..per_chunk {
            let size = *sizes
                .get(sample)
                .ok_or_else(|| bad("more samples in chunks than in stsz"))?;
            out.push((at, size));
            at += u64::from(size);
            sample += 1;
        }
    }
    if sample != count {
        return Err(bad("chunk tables do not cover every sample"));
    }
    Ok(out)
}

/// The video track as Annex B access units again: each sample's length-prefixed NAL units with start codes, the sync
/// samples with the SPS and PPS of the sample entry in front. What a decoder is fed with.
pub fn read_video_units(file: &mut (impl Read + Seek)) -> io::Result<Vec<Vec<u8>>> {
    let size = file.seek(SeekFrom::End(0))?;
    let mut at = 0u64;
    while at < size {
        file.seek(SeekFrom::Start(at))?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let length = u64::from(u32::from_be_bytes(header[..4].try_into().unwrap()));
        if length < 8 || at + length > size {
            return Err(bad("top-level box size out of range"));
        }
        if &header[4..8] == b"moov" {
            let mut body = vec![0u8; (length - 8) as usize];
            file.read_exact(&mut body)?;
            for (kind, track) in children(&body)? {
                if &kind != b"trak" {
                    continue;
                }
                let trak = children(track)?;
                let mdia = children(find(&trak, b"mdia").ok_or_else(|| bad("no mdia"))?)?;
                let hdlr = find(&mdia, b"hdlr").ok_or_else(|| bad("no hdlr"))?;
                if hdlr.get(8..12) != Some(b"vide") {
                    continue;
                }
                let minf = children(find(&mdia, b"minf").ok_or_else(|| bad("no minf"))?)?;
                let stbl = children(find(&minf, b"stbl").ok_or_else(|| bad("no stbl"))?)?;
                let stsd = find(&stbl, b"stsd").ok_or_else(|| bad("no stsd"))?;
                // 8 bytes of the stsd header, then the sample entry: its own 8 and the visual entry's 78 fields.
                let entry = children(stsd.get(8..).ok_or_else(|| bad("short stsd"))?)?;
                let avc1 = entry.first().ok_or_else(|| bad("no sample entry"))?.1;
                let inner = children(avc1.get(78..).ok_or_else(|| bad("short avc1"))?)?;
                let avcc = find(&inner, b"avcC").ok_or_else(|| bad("no avcC"))?;
                let sps_len = u16::from_be_bytes([avcc[6], avcc[7]]) as usize;
                let sps = &avcc[8..8 + sps_len];
                let pps_len = u16::from_be_bytes([avcc[9 + sps_len], avcc[10 + sps_len]]) as usize;
                let pps = &avcc[11 + sps_len..11 + sps_len + pps_len];
                let sync: Option<Vec<u32>> = match find(&stbl, b"stss") {
                    Some(stss) => {
                        let count = be32(stss, 4)? as usize;
                        Some(
                            (0..count)
                                .map(|i| be32(stss, 8 + i * 4))
                                .collect::<io::Result<_>>()?,
                        )
                    }
                    None => None,
                };
                let mut units = Vec::new();
                for (index, (offset, length)) in sample_locations(&stbl)?.into_iter().enumerate() {
                    file.seek(SeekFrom::Start(offset))?;
                    let mut sample = vec![0u8; length as usize];
                    file.read_exact(&mut sample)?;
                    let mut unit = Vec::new();
                    if sync
                        .as_ref()
                        .is_none_or(|list| list.contains(&(index as u32 + 1)))
                    {
                        for nal in [sps, pps] {
                            unit.extend_from_slice(&[0, 0, 0, 1]);
                            unit.extend_from_slice(nal);
                        }
                    }
                    let mut rest = sample.as_slice();
                    while rest.len() >= 4 {
                        let nal_len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
                        let nal = rest
                            .get(4..4 + nal_len)
                            .ok_or_else(|| bad("sample NAL runs past its end"))?;
                        unit.extend_from_slice(&[0, 0, 0, 1]);
                        unit.extend_from_slice(nal);
                        rest = &rest[4 + nal_len..];
                    }
                    units.push(unit);
                }
                return Ok(units);
            }
            return Err(bad("no video track"));
        }
        at += length;
    }
    Err(bad("no moov box"))
}

// ----- reading a clip back whole -----

/// A clip file read back: its video and audio as the writer takes them, and its metadata document. What the editor
/// plays and what a trimmed copy is cut from.
pub struct ClipFile {
    pub video: VideoInput,
    pub audio: Vec<AudioInput>,
    pub metadata: Option<Vec<u8>>,
}

fn stts_deltas(stbl: &[([u8; 4], &[u8])], samples: usize) -> io::Result<Vec<u32>> {
    let stts = find(stbl, b"stts").ok_or_else(|| bad("track without stts"))?;
    let entries = be32(stts, 4)? as usize;
    let mut out = Vec::with_capacity(samples);
    for index in 0..entries {
        let (count, delta) = (be32(stts, 8 + index * 8)?, be32(stts, 12 + index * 8)?);
        for _ in 0..count {
            out.push(delta);
        }
    }
    if out.len() != samples {
        return Err(bad("stts does not cover every sample"));
    }
    Ok(out)
}

fn read_samples(file: &mut (impl Read + Seek), places: &[(u64, u32)]) -> io::Result<Vec<Vec<u8>>> {
    let mut out = Vec::with_capacity(places.len());
    for (offset, length) in places {
        file.seek(SeekFrom::Start(*offset))?;
        let mut sample = vec![0u8; *length as usize];
        file.read_exact(&mut sample)?;
        out.push(sample);
    }
    Ok(out)
}

/// Reads a whole clip file (the video track, every audio track and the metadata document).
pub fn read_clip(file: &mut (impl Read + Seek)) -> io::Result<ClipFile> {
    let info = read_info(file)?;
    let size = file.seek(SeekFrom::End(0))?;
    let mut at = 0u64;
    let mut video = None;
    let mut audio = Vec::new();
    while at < size {
        file.seek(SeekFrom::Start(at))?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let length = u64::from(u32::from_be_bytes(header[..4].try_into().unwrap()));
        if length < 8 || at + length > size {
            return Err(bad("top-level box size out of range"));
        }
        if &header[4..8] != b"moov" {
            at += length;
            continue;
        }
        let mut body = vec![0u8; (length - 8) as usize];
        file.read_exact(&mut body)?;
        for (kind, track) in children(&body)? {
            if &kind != b"trak" {
                continue;
            }
            let trak = children(track)?;
            let mdia = children(find(&trak, b"mdia").ok_or_else(|| bad("no mdia"))?)?;
            let hdlr = find(&mdia, b"hdlr").ok_or_else(|| bad("no hdlr"))?;
            let handler = hdlr.get(8..12).ok_or_else(|| bad("short hdlr"))?;
            let name = String::from_utf8_lossy(
                hdlr.get(24..)
                    .unwrap_or(&[])
                    .split(|b| *b == 0)
                    .next()
                    .unwrap_or(&[]),
            )
            .into_owned();
            let minf = children(find(&mdia, b"minf").ok_or_else(|| bad("no minf"))?)?;
            let stbl = children(find(&minf, b"stbl").ok_or_else(|| bad("no stbl"))?)?;
            let stsd = find(&stbl, b"stsd").ok_or_else(|| bad("no stsd"))?;
            let entries = children(stsd.get(8..).ok_or_else(|| bad("short stsd"))?)?;
            let entry_pair = *entries.first().ok_or_else(|| bad("no sample entry"))?;
            let (entry_type, entry) = entry_pair;
            let places = sample_locations(&stbl)?;
            match handler {
                b"vide" => {
                    let inner = children(entry.get(78..).ok_or_else(|| bad("short avc1"))?)?;
                    let avcc = find(&inner, b"avcC").ok_or_else(|| bad("no avcC"))?;
                    let sps_len = u16::from_be_bytes([avcc[6], avcc[7]]) as usize;
                    let sps = avcc[8..8 + sps_len].to_vec();
                    let pps_len =
                        u16::from_be_bytes([avcc[9 + sps_len], avcc[10 + sps_len]]) as usize;
                    let pps = avcc[11 + sps_len..11 + sps_len + pps_len].to_vec();
                    let deltas = stts_deltas(&stbl, places.len())?;
                    let sync: Option<Vec<u32>> = match find(&stbl, b"stss") {
                        Some(stss) => {
                            let count = be32(stss, 4)? as usize;
                            Some(
                                (0..count)
                                    .map(|i| be32(stss, 8 + i * 4))
                                    .collect::<io::Result<_>>()?,
                            )
                        }
                        None => None,
                    };
                    let data = read_samples(file, &places)?;
                    let samples = data
                        .into_iter()
                        .zip(deltas)
                        .enumerate()
                        .map(|(index, (data, duration))| VideoSample {
                            data,
                            duration,
                            sync: sync
                                .as_ref()
                                .is_none_or(|list| list.contains(&(index as u32 + 1))),
                        })
                        .collect();
                    let sps_info = crate::sps::parse(&sps).ok_or_else(|| bad("unreadable SPS"))?;
                    video = Some(VideoInput {
                        sps,
                        pps,
                        info: sps_info,
                        samples,
                    });
                }
                b"soun" if &entry_type == b"Opus" => {
                    let channels = entry
                        .get(16..18)
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                        .ok_or_else(|| bad("short audio entry"))?
                        as u8;
                    let inner = children(entry.get(28..).unwrap_or(&[]))?;
                    let pre_skip = find(&inner, b"dOps")
                        .and_then(|dops| dops.get(2..4))
                        .map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
                    audio.push(AudioInput {
                        name,
                        channels,
                        pre_skip,
                        packets: read_samples(file, &places)?,
                    });
                }
                _ => {}
            }
        }
        break;
    }
    Ok(ClipFile {
        video: video.ok_or_else(|| bad("no video track"))?,
        audio,
        metadata: info.metadata,
    })
}

impl ClipFile {
    /// The start of each video sample in milliseconds, and the clip's length.
    pub fn video_starts_ms(&self) -> (Vec<u64>, u64) {
        let mut ticks = 0u64;
        let mut starts = Vec::with_capacity(self.video.samples.len());
        for sample in &self.video.samples {
            starts.push(ticks * 1000 / u64::from(VIDEO_TIMESCALE));
            ticks += u64::from(sample.duration);
        }
        (starts, ticks * 1000 / u64::from(VIDEO_TIMESCALE))
    }

    /// The part of the clip from `start_ms` to `end_ms`. The video begins at the last keyframe at or before the
    /// start (a picture cannot be decoded without it), so a copy may run up to one group of pictures longer at the
    /// front than asked; it ends with the last picture that starts before `end_ms`. The audio follows the same
    /// instants. `None` when the range holds no picture.
    pub fn trimmed(&self, start_ms: u64, end_ms: u64) -> Option<ClipFile> {
        let (starts, _) = self.video_starts_ms();
        let first = starts
            .iter()
            .enumerate()
            .filter(|(index, start)| **start <= start_ms && self.video.samples[*index].sync)
            .map(|(index, _)| index)
            .next_back()
            .or_else(|| self.video.samples.iter().position(|sample| sample.sync))?;
        let last = starts.iter().rposition(|start| *start < end_ms)?;
        if last < first {
            return None;
        }
        let samples: Vec<VideoSample> = self.video.samples[first..=last]
            .iter()
            .map(|sample| VideoSample {
                data: sample.data.clone(),
                duration: sample.duration,
                sync: sample.sync,
            })
            .collect();
        let lead_ms = starts[first];
        let length_ms: u64 = samples
            .iter()
            .map(|sample| u64::from(sample.duration))
            .sum::<u64>()
            * 1000
            / u64::from(VIDEO_TIMESCALE);
        let packet_ms = u64::from(AUDIO_PACKET_TICKS) * 1000 / u64::from(AUDIO_TIMESCALE);
        let from = (lead_ms / packet_ms) as usize;
        let to = ((lead_ms + length_ms).div_ceil(packet_ms)) as usize;
        Some(ClipFile {
            video: VideoInput {
                sps: self.video.sps.clone(),
                pps: self.video.pps.clone(),
                info: self.video.info,
                samples,
            },
            audio: self
                .audio
                .iter()
                .map(|track| AudioInput {
                    name: track.name.clone(),
                    channels: track.channels,
                    pre_skip: track.pre_skip,
                    packets: track
                        .packets
                        .get(from.min(track.packets.len())..to.min(track.packets.len()))
                        .unwrap_or(&[])
                        .to_vec(),
                })
                .collect(),
            metadata: self.metadata.clone(),
        })
    }

    /// Drops the audio tracks whose names are not in `keep` (the editor's audio switches).
    pub fn keep_audio(&mut self, keep: &[&str]) {
        self.audio
            .retain(|track| keep.contains(&track.name.as_str()));
    }

    /// Decodes matching audio tracks (`None` means all tracks) and mixes them down to mono 48 kHz PCM.
    pub fn decode_mixed_mono(&self, keep: Option<&[&str]>) -> Vec<f32> {
        let mut mixed: Vec<f32> = Vec::new();
        for track in &self.audio {
            if keep.is_some_and(|names| !names.contains(&track.name.as_str())) {
                continue;
            }
            let pcm = crate::audio::decode_track_mono(track.channels, track.pre_skip, &track.packets);
            accumulate_pcm(&mut mixed, &pcm);
        }
        clamp_pcm(&mut mixed);
        mixed
    }

    /// Folds multiple audio tracks down into a single stereo Opus track so standard players hear all kept tracks.
    pub fn fold_audio_to_single_stereo(&mut self) {
        if self.audio.len() <= 1 {
            return;
        }
        let mut mixed: Vec<f32> = Vec::new();
        for track in &self.audio {
            let pcm =
                crate::audio::decode_track_stereo(track.channels, track.pre_skip, &track.packets);
            accumulate_pcm(&mut mixed, &pcm);
        }
        clamp_pcm(&mut mixed);
        let (pre_skip, packets) = crate::audio::encode_stereo_packets(&mixed);
        if !packets.is_empty() {
            self.audio = vec![AudioInput {
                name: "application".to_owned(),
                channels: 2,
                pre_skip,
                packets,
            }];
        }
    }

    /// Writes the clip with `metadata` as its document. Returns the size.
    pub fn write(&self, out: &mut impl Write, metadata: &[u8]) -> io::Result<u64> {
        write(out, &self.video, &self.audio, metadata)
    }
}

fn accumulate_pcm(mixed: &mut Vec<f32>, pcm: &[f32]) {
    if pcm.len() > mixed.len() {
        mixed.resize(pcm.len(), 0.0);
    }
    for (dst, src) in mixed.iter_mut().zip(pcm) {
        *dst += *src;
    }
}

fn clamp_pcm(samples: &mut [f32]) {
    for sample in samples {
        *sample = sample.clamp(-1.0, 1.0);
    }
}

/// Replaces the metadata document of a clip file in place: the document is the last box, so the file is cut where
/// it starts and the new one appended. Written through a temporary file next to it and renamed over it, so a failure
/// leaves the original whole.
pub fn replace_metadata(path: &std::path::Path, metadata: &[u8]) -> io::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let size = file.seek(SeekFrom::End(0))?;
    let mut at = 0u64;
    let mut cut = size;
    while at < size {
        file.seek(SeekFrom::Start(at))?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let length = u64::from(u32::from_be_bytes(header[..4].try_into().unwrap()));
        if length < 8 || at + length > size {
            return Err(bad("top-level box size out of range"));
        }
        if &header[4..8] == b"uuid" && length >= 24 {
            let mut user = [0u8; 16];
            file.read_exact(&mut user)?;
            if is_metadata_uuid(&user) {
                cut = at;
            }
        }
        at += length;
    }
    file.seek(SeekFrom::Start(0))?;
    let temp = path.with_extension("mp4.part");
    let result = (|| {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&temp)?);
        std::io::copy(&mut (&mut file).take(cut), &mut out)?;
        let mut payload = METADATA_UUID.to_vec();
        payload.extend_from_slice(metadata);
        out.write_all(&bx(b"uuid", &payload))?;
        out.flush()
    })();
    drop(file);
    match result {
        Ok(()) => std::fs::rename(&temp, path),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::TrackEncoder;
    use crate::buffer::PACKET_SAMPLES;
    use std::io::Cursor;

    fn sine_samples(frames: usize, channels: usize, freq_hz: f32, amp: f32) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * channels);
        for i in 0..frames {
            let s = (2.0 * std::f32::consts::PI * freq_hz * i as f32 / 48_000.0).sin() * amp;
            for _ in 0..channels {
                out.push(s);
            }
        }
        out
    }

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = samples.iter().map(|s| s * s).sum();
        (sum / samples.len() as f32).sqrt()
    }

    #[test]
    fn multi_track_clip_decodes_and_folds_to_single_stereo_track() {
        let mut generator = dc_media::synthetic::Generator::new(320, 180, 10, 400_000).unwrap();
        let frame = generator.next(true).unwrap().unwrap();
        let sps = dc_media::h264::nals(&frame.data)
            .find(|n| n[0] & 31 == 7)
            .expect("keyframe has sps")
            .to_vec();
        let pps = dc_media::h264::nals(&frame.data)
            .find(|n| n[0] & 31 == 8)
            .expect("keyframe has pps")
            .to_vec();
        let info = crate::sps::parse(&sps).unwrap();
        let video = VideoInput {
            sps,
            pps,
            info,
            samples: vec![VideoSample {
                data: to_length_prefixed(&frame.data),
                duration: VIDEO_TIMESCALE / 5,
                sync: true,
            }],
        };

        let samples_total = PACKET_SAMPLES * 10;
        let mut app_enc = TrackEncoder::new(2).unwrap();
        let app_skip = app_enc.pre_skip();
        let app_packets: Vec<Vec<u8>> = app_enc
            .push(&sine_samples(samples_total, 2, 440.0, 0.25), 200)
            .into_iter()
            .map(|p| p.data)
            .collect();

        let mut voice_enc = TrackEncoder::new(1).unwrap();
        let voice_skip = voice_enc.pre_skip();
        let voice_packets: Vec<Vec<u8>> = voice_enc
            .push(&sine_samples(samples_total, 1, 660.0, 0.30), 200)
            .into_iter()
            .map(|p| p.data)
            .collect();

        let audio = vec![
            AudioInput {
                name: "application".to_owned(),
                channels: 2,
                pre_skip: app_skip,
                packets: app_packets,
            },
            AudioInput {
                name: "voice".to_owned(),
                channels: 1,
                pre_skip: voice_skip,
                packets: voice_packets,
            },
        ];

        let mut raw = Vec::new();
        write(&mut raw, &video, &audio, &[]).unwrap();
        let mut read_back = read_clip(&mut Cursor::new(&raw)).unwrap();
        assert_eq!(read_back.audio.len(), 2);

        let only_app = read_back.decode_mixed_mono(Some(&["application"]));
        let only_voice = read_back.decode_mixed_mono(Some(&["voice"]));
        let both = read_back.decode_mixed_mono(None);
        assert!(!only_app.is_empty());
        assert!(!only_voice.is_empty());
        assert!(rms(&both) > rms(&only_app) * 0.9);
        assert!(rms(&only_app) > 0.05);
        assert!(rms(&only_voice) > 0.05);
        assert_ne!(only_app, both);

        read_back.fold_audio_to_single_stereo();
        assert_eq!(read_back.audio.len(), 1);
        assert_eq!(read_back.audio[0].channels, 2);
        assert!(!read_back.audio[0].packets.is_empty());

        let mut folded_raw = Vec::new();
        read_back.write(&mut folded_raw, &[]).unwrap();
        let folded = read_clip(&mut Cursor::new(&folded_raw)).unwrap();
        assert_eq!(folded.audio.len(), 1);
        let folded_mono = folded.decode_mixed_mono(None);
        assert!(rms(&folded_mono) > 0.05);
    }
}
