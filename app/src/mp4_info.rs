//! What "Stats for Nerds" (module 410694) says about a video file: the fields module 265486 `K` reads with mp4box,
//! taken from the file's `ftyp` and `moov` boxes. A file that cannot be read gives every field `None`, as `K`'s
//! fallback does.

use crate::spinner::{be32, be64, boxes, child, path};

/// The codec information of module 265486 (`videoCodec` .. `containerFormat`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CodecInfo {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub video_codec_description: Option<String>,
    pub audio_codec_description: Option<String>,
    pub video_bitrate: Option<f64>,
    pub audio_bitrate: Option<f64>,
    pub audio_channels: Option<u32>,
    pub audio_sample_rate: Option<u32>,
    pub frame_rate: Option<u32>,
    pub video_width: Option<u32>,
    pub video_height: Option<u32>,
    pub is_progressive: Option<bool>,
    pub is_fragmented: Option<bool>,
    pub container_format: Option<String>,
}

/// One track as mp4box's `getInfo` describes it.
struct Track {
    handler: [u8; 4],
    codec: String,
    timescale: u64,
    duration: u64,
    samples: u64,
    samples_size: u64,
    samples_duration: u64,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u32>,
    sample_rate: Option<u32>,
}

impl Track {
    /// mp4box: `size * 8 * timescale / samples_duration`.
    fn bitrate(&self) -> Option<f64> {
        (self.samples_duration > 0).then(|| {
            (self.samples_size as f64 * 8.0 * self.timescale as f64) / self.samples_duration as f64
        })
    }

    /// Module 265486: `nb_samples / (duration / timescale)`, rounded.
    fn frame_rate(&self) -> Option<u32> {
        if self.timescale == 0 {
            return None;
        }
        let seconds = self.duration as f64 / self.timescale as f64;
        (seconds > 0.0).then(|| (self.samples as f64 / seconds).round() as u32)
    }
}

/// Reads the codec information of an MP4 file.
pub(crate) fn read(file: &[u8]) -> CodecInfo {
    let top = boxes(file);
    let Some(moov) = top.iter().position(|entry| &entry.kind == b"moov") else {
        return CodecInfo::default();
    };
    // mp4box: a file is progressive when its `moov` comes before any `mdat`.
    let progressive = !top[..moov].iter().any(|entry| &entry.kind == b"mdat");
    let moov = top[moov].payload;
    let brands = top
        .iter()
        .find(|entry| &entry.kind == b"ftyp")
        .map(|ftyp| brands(ftyp.payload))
        .unwrap_or_default();
    let tracks: Vec<Track> = boxes(moov)
        .into_iter()
        .filter(|entry| &entry.kind == b"trak")
        .filter_map(|trak| track(trak.payload))
        .collect();
    let video = tracks.iter().find(|track| &track.handler == b"vide");
    let audio = tracks.iter().find(|track| &track.handler == b"soun");
    CodecInfo {
        video_codec: video.map(|track| track.codec.clone()),
        audio_codec: audio.map(|track| track.codec.clone()),
        video_codec_description: video.map(|track| video_description(&track.codec)),
        audio_codec_description: audio.map(|track| audio_description(&track.codec)),
        video_bitrate: video.and_then(Track::bitrate),
        audio_bitrate: audio.and_then(Track::bitrate),
        audio_channels: audio.and_then(|track| track.channels),
        audio_sample_rate: audio.and_then(|track| track.sample_rate),
        frame_rate: video.and_then(Track::frame_rate),
        video_width: video.and_then(|track| track.width),
        video_height: video.and_then(|track| track.height),
        is_progressive: Some(progressive),
        is_fragmented: Some(child(moov, b"mvex").is_some()),
        container_format: Some(container_format(&brands)),
    }
}

/// mp4box's `brands`: the major brand, then the compatible ones.
fn brands(ftyp: &[u8]) -> Vec<String> {
    let brand = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let mut out = Vec::new();
    if let Some(major) = ftyp.get(..4) {
        out.push(brand(major));
    }
    if let Some(rest) = ftyp.get(8..) {
        out.extend(rest.as_chunks::<4>().0.iter().map(|chunk| brand(chunk)));
    }
    out
}

fn track(trak: &[u8]) -> Option<Track> {
    let mdia = child(trak, b"mdia")?;
    let hdlr = child(mdia, b"hdlr")?;
    let handler: [u8; 4] = hdlr.get(8..12)?.try_into().ok()?;
    let mdhd = child(mdia, b"mdhd")?;
    let (timescale, duration) = if mdhd.first() == Some(&1) {
        (u64::from(be32(mdhd.get(20..)?)?), be64(mdhd.get(24..)?)?)
    } else {
        (
            u64::from(be32(mdhd.get(12..)?)?),
            u64::from(be32(mdhd.get(16..)?)?),
        )
    };
    let stbl = path(mdia, &[b"minf", b"stbl"])?;
    let entry = boxes(child(stbl, b"stsd")?.get(8..)?).into_iter().next()?;
    let (samples, samples_size) = child(stbl, b"stsz").map_or((0, 0), sample_sizes);
    let samples_duration = child(stbl, b"stts").map_or(0, sample_durations);
    let kind = String::from_utf8_lossy(&entry.kind).into_owned();
    let mut track = Track {
        handler,
        codec: kind.clone(),
        timescale,
        duration,
        samples,
        samples_size,
        samples_duration,
        width: None,
        height: None,
        channels: None,
        sample_rate: None,
    };
    let body = entry.payload;
    if &handler == b"vide" {
        track.width = body
            .get(24..26)
            .map(|bytes| u32::from(u16::from_be_bytes([bytes[0], bytes[1]])));
        track.height = body
            .get(26..28)
            .map(|bytes| u32::from(u16::from_be_bytes([bytes[0], bytes[1]])));
        let children = body.get(78..).unwrap_or_default();
        track.codec = video_codec(&kind, children);
    } else if &handler == b"soun" {
        track.channels = body
            .get(16..18)
            .map(|bytes| u32::from(u16::from_be_bytes([bytes[0], bytes[1]])));
        track.sample_rate = body.get(24..).and_then(be32).map(|rate| rate >> 16);
        // QuickTime sound descriptions add 16 (version 1) or 36 (version 2) bytes before the child boxes.
        let extra = match body
            .get(8..10)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
        {
            Some(1) => 16,
            Some(2) => 36,
            _ => 0,
        };
        let children = body.get(28 + extra..).unwrap_or_default();
        track.codec = audio_codec(&kind, children);
    }
    Some(track)
}

/// `stsz`: the sample count and the sum of the sample sizes.
fn sample_sizes(stsz: &[u8]) -> (u64, u64) {
    let fixed = stsz.get(4..).and_then(be32).unwrap_or(0);
    let count = stsz.get(8..).and_then(be32).unwrap_or(0);
    if fixed != 0 {
        return (u64::from(count), u64::from(fixed) * u64::from(count));
    }
    let sizes = stsz.get(12..).unwrap_or_default();
    let total = sizes
        .as_chunks::<4>()
        .0
        .iter()
        .take(count as usize)
        .map(|size| u64::from(u32::from_be_bytes(*size)))
        .sum();
    (u64::from(count), total)
}

/// `stts`: the sum of every sample's duration.
fn sample_durations(stts: &[u8]) -> u64 {
    let count = stts.get(4..).and_then(be32).unwrap_or(0) as usize;
    stts.get(8..)
        .unwrap_or_default()
        .as_chunks::<8>()
        .0
        .iter()
        .take(count)
        .filter_map(|entry| Some(u64::from(be32(entry)?) * u64::from(be32(&entry[4..])?)))
        .sum()
}

/// mp4box `getCodec` of a visual sample entry.
fn video_codec(kind: &str, children: &[u8]) -> String {
    match kind {
        "avc1" | "avc2" | "avc3" | "avc4" => {
            match child(children, b"avcC").and_then(|avcc| avcc.get(1..4)) {
                Some(bytes) => format!("{kind}.{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2]),
                None => kind.to_owned(),
            }
        }
        "hvc1" | "hev1" => child(children, b"hvcC")
            .and_then(|hvcc| hevc_codec(kind, hvcc))
            .unwrap_or_else(|| kind.to_owned()),
        "vp08" | "vp09" => child(children, b"vpcC")
            .and_then(|vpcc| {
                let profile = *vpcc.get(4)?;
                let level = *vpcc.get(5)?;
                let depth = *vpcc.get(6)? >> 4;
                Some(format!("{kind}.{profile:02}.{level:02}.{depth:02}"))
            })
            .unwrap_or_else(|| kind.to_owned()),
        "av01" => child(children, b"av1C")
            .and_then(|av1c| {
                let profile = *av1c.get(1)? >> 5;
                let level = *av1c.get(1)? & 0x1f;
                let flags = *av1c.get(2)?;
                let tier = if flags & 0x80 != 0 { 'H' } else { 'M' };
                let depth = if flags & 0x40 == 0 {
                    8
                } else if flags & 0x20 != 0 {
                    12
                } else {
                    10
                };
                Some(format!("av01.{profile}.{level:02}{tier}.{depth:02}"))
            })
            .unwrap_or_else(|| kind.to_owned()),
        _ => kind.to_owned(),
    }
}

/// mp4box's HEVC codec string: profile space and idc, the reversed compatibility flags, tier and level, then the
/// constraint bytes up to the last one that is set.
fn hevc_codec(kind: &str, hvcc: &[u8]) -> Option<String> {
    let first = *hvcc.get(1)?;
    let space = ["", "A", "B", "C"][usize::from(first >> 6)];
    let tier = if first & 0x20 != 0 { 'H' } else { 'L' };
    let profile = first & 0x1f;
    let compatibility = be32(hvcc.get(2..)?)?.reverse_bits();
    let constraints = hvcc.get(6..12)?;
    let level = *hvcc.get(12)?;
    let last = constraints.iter().rposition(|byte| *byte != 0);
    let tail: String = last.map_or(String::new(), |last| {
        constraints[..=last]
            .iter()
            .map(|byte| format!(".{byte:x}"))
            .collect()
    });
    Some(format!(
        "{kind}.{space}{profile}.{compatibility:x}.{tier}{level}{tail}"
    ))
}

/// mp4box `getCodec` of a sound sample entry: `mp4a.<object type>.<audio object type>` from `esds`.
fn audio_codec(kind: &str, children: &[u8]) -> String {
    if kind != "mp4a" {
        return kind.to_owned();
    }
    let Some(esds) = child(children, b"esds").and_then(|esds| esds.get(4..)) else {
        return kind.to_owned();
    };
    match es_config(esds) {
        Some((object, Some(audio))) => format!("mp4a.{object:02x}.{audio}"),
        Some((object, None)) => format!("mp4a.{object:02x}"),
        None => kind.to_owned(),
    }
}

/// The object type of an `ES_Descriptor`'s decoder config and the audio object type of its specific info.
fn es_config(data: &[u8]) -> Option<(u8, Option<u32>)> {
    let (tag, body) = descriptor(data)?;
    if tag != 0x03 {
        return None;
    }
    let flags = *body.get(2)?;
    let mut at = 3;
    if flags & 0x80 != 0 {
        at += 2;
    }
    if flags & 0x40 != 0 {
        at += 1 + usize::from(*body.get(at)?);
    }
    if flags & 0x20 != 0 {
        at += 2;
    }
    let (tag, config) = descriptor(body.get(at..)?)?;
    if tag != 0x04 {
        return None;
    }
    let object = *config.first()?;
    let audio = config
        .get(13..)
        .and_then(descriptor)
        .and_then(|(tag, info)| {
            if tag != 0x05 {
                return None;
            }
            let first = u32::from(*info.first()?);
            let kind = (first & 0xf8) >> 3;
            if kind == 31 {
                let second = u32::from(*info.get(1)?);
                Some(32 + ((first & 7) << 3) + ((second & 0xe0) >> 5))
            } else {
                Some(kind)
            }
        });
    Some((object, audio))
}

/// One MPEG-4 descriptor: its tag and body (the size is 7 bits a byte, the high bit continuing it).
fn descriptor(data: &[u8]) -> Option<(u8, &[u8])> {
    let tag = *data.first()?;
    let mut size = 0usize;
    let mut at = 1;
    loop {
        let byte = *data.get(at)?;
        at += 1;
        size = (size << 7) | usize::from(byte & 0x7f);
        if byte & 0x80 == 0 || at > 4 {
            break;
        }
    }
    Some((tag, data.get(at..at + size)?))
}

/// Module 265486: the name of a video codec string.
fn video_description(codec: &str) -> String {
    let known = [
        ("avc1", "H.264/AVC"),
        ("hev1", "H.265/HEVC"),
        ("hvc1", "H.265/HEVC"),
        ("vp08", "VP8"),
        ("vp09", "VP9"),
        ("av01", "AV1"),
    ];
    known
        .iter()
        .find(|(prefix, _)| codec.starts_with(prefix))
        .map_or_else(|| codec.to_owned(), |(_, name)| (*name).to_owned())
}

/// Module 265486: the name of an audio codec string.
fn audio_description(codec: &str) -> String {
    let name = if codec.starts_with("mp4a.40.2") {
        "AAC-LC"
    } else if codec.starts_with("mp4a.40.5") {
        "HE-AAC"
    } else if codec.starts_with("mp4a.40.29") {
        "HE-AACv2"
    } else if codec.starts_with("mp4a.40") {
        "AAC"
    } else if codec == "opus" {
        "Opus"
    } else if codec == "vorbis" {
        "Vorbis"
    } else {
        return codec.to_owned();
    };
    name.to_owned()
}

/// Module 265486: the name of the container by its major brand.
fn container_format(brands: &[String]) -> String {
    let Some(brand) = brands.first() else {
        return "MP4".to_owned();
    };
    match brand.as_str() {
        "isom" => "MP4 (ISO Base Media)".to_owned(),
        "mp41" => "MP4 v1".to_owned(),
        "mp42" => "MP4 v2".to_owned(),
        _ if brand.starts_with("M4V") => "M4V (iTunes Video)".to_owned(),
        _ if brand.starts_with("M4A") => "M4A (iTunes Audio)".to_owned(),
        _ if brand.starts_with("qt") => "QuickTime".to_owned(),
        "dash" => "DASH".to_owned(),
        "iso5" => "MP4 (ISO/IEC 14496-12:2005)".to_owned(),
        "iso6" => "MP4 (ISO/IEC 14496-12:2012)".to_owned(),
        _ => format!("MP4 ({brand})"),
    }
}

/// Module 265486 `BZ`: a bit rate.
pub(crate) fn bitrate_text(bits: f64) -> String {
    if bits < 1e3 {
        format!("{bits} bps")
    } else if bits < 1e6 {
        format!("{:.1} Kbps", bits / 1e3)
    } else {
        format!("{:.2} Mbps", bits / 1e6)
    }
}

/// Module 265486 `At`: a channel count.
pub(crate) fn channels_text(channels: u32) -> String {
    match channels {
        1 => "Mono".to_owned(),
        2 => "Stereo".to_owned(),
        6 => "5.1 Surround".to_owned(),
        8 => "7.1 Surround".to_owned(),
        other => format!("{other} channels"),
    }
}

/// Module 265486 `YQ`: a sample rate.
pub(crate) fn sample_rate_text(rate: u32) -> String {
    if rate < 1000 {
        format!("{rate} Hz")
    } else {
        format!("{:.1} kHz", f64::from(rate) / 1e3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_synthetic_h264_file_reads_like_mp4box() {
        let mp4 =
            crate::video_vod::synthetic_mp4(64, 48, 2, &[[255, 0, 0], [0, 255, 0]], 2).unwrap();
        let info = read(&mp4);
        assert_eq!(info.video_width, Some(64));
        assert_eq!(info.video_height, Some(48));
        assert_eq!(info.video_codec_description.as_deref(), Some("H.264/AVC"));
        assert!(
            info.video_codec
                .as_deref()
                .is_some_and(|codec| codec.starts_with("avc1."))
        );
        assert_eq!(info.audio_codec, None);
        assert_eq!(info.is_fragmented, Some(false));
        assert!(info.container_format.is_some());
        assert!(info.video_bitrate.is_some_and(|bits| bits > 0.0));
    }

    #[test]
    fn unreadable_bytes_give_no_fields() {
        assert_eq!(read(b"not a video"), CodecInfo::default());
    }

    #[test]
    fn texts_follow_module_265486() {
        assert_eq!(bitrate_text(800.0), "800 bps");
        assert_eq!(bitrate_text(128_000.0), "128.0 Kbps");
        assert_eq!(bitrate_text(2_345_678.0), "2.35 Mbps");
        assert_eq!(channels_text(2), "Stereo");
        assert_eq!(channels_text(3), "3 channels");
        assert_eq!(sample_rate_text(48_000), "48.0 kHz");
        assert_eq!(
            container_format(&["isom".to_owned()]),
            "MP4 (ISO Base Media)"
        );
        assert_eq!(container_format(&[]), "MP4");
        assert_eq!(audio_description("mp4a.40.2"), "AAC-LC");
        // Module 265486 tests "mp4a.40.2" first, so HE-AACv2's string reads AAC-LC there too.
        assert_eq!(audio_description("mp4a.40.29"), "AAC-LC");
        assert_eq!(audio_description("mp4a.40.5"), "HE-AAC");
    }

    #[test]
    fn hevc_and_aac_codec_strings() {
        // hvcC: version, Main profile (1) tier L, compatibility 0x60000000, constraint 0x90, level 93.
        let hvcc = [1, 0x01, 0x60, 0, 0, 0, 0x90, 0, 0, 0, 0, 0, 93];
        assert_eq!(
            hevc_codec("hvc1", &hvcc).as_deref(),
            Some("hvc1.1.6.L93.90")
        );
        // ES_Descriptor > DecoderConfig (object 0x40) > DecoderSpecificInfo (AAC-LC, 2).
        let mut config = vec![0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        config.extend([0x05, 2, 0x12, 0x10]);
        let mut es = vec![0, 1, 0];
        es.push(0x04);
        es.push(config.len() as u8);
        es.extend(&config);
        let mut esds = vec![0, 0, 0, 0, 0x03, es.len() as u8];
        esds.extend(&es);
        let mut children = ((esds.len() + 8) as u32).to_be_bytes().to_vec();
        children.extend(b"esds");
        children.extend(&esds);
        assert_eq!(audio_codec("mp4a", &children), "mp4a.40.2");
    }
}
