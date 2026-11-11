//! A clip from the real encoder's output: the buffer is fed on a simulated clock, a window is cut, written, read back with
//! this crate's reader and decoded with the same decoder the stream viewers use.
use dc_clips::{
    ClipMetadata, SnapshotError, SourceInfo, Track, library, metadata::file_name, mp4, standalone,
};
use dc_media::{decode::Decoder, synthetic::Generator};
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-clips-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Twelve seconds of a 10 fps stream with a tone on the stream audio and a quieter one on the voice, on a clock that
/// starts at 1 000 ms. Returns the end time.
fn feed(recorder: &'static dc_clips::Recorder, length_ms: u64, seconds: u64) -> u64 {
    recorder.configure(true, length_ms);
    recorder.announce_source(SourceInfo {
        name: "Test Pattern".into(),
        application_id: None,
    });
    let tap = recorder.attach();
    let mut generator = Generator::new(320, 180, 10, 300_000).unwrap();
    let frames = seconds * 10;
    for index in 0..frames {
        let now = 1_000 + index * 100;
        let frame = generator
            .next(false)
            .unwrap()
            .expect("every picture comes out");
        tap.push_video_at(&frame, now);
        // 100 ms of stereo stream audio, and of mono voice except for a quiet second in the middle.
        let tone = |channels: usize, base: u64| -> Vec<f32> {
            (0..4_800 * channels)
                .map(|n| ((((base * 4_800 + (n / channels) as u64) as f32) * 0.05).sin()) * 0.3)
                .collect()
        };
        tap.push_audio_at(Track::Application, &tone(2, index), now);
        if !(5..6).contains(&(index / 10)) {
            tap.push_audio_at(Track::Voice, &tone(1, index), now);
        }
    }
    1_000 + (frames - 1) * 100
}

#[test]
fn a_cut_window_is_written_read_back_and_decoded() {
    let recorder = standalone();
    let end = feed(recorder, 5_000, 12);
    let snapshot = recorder.snapshot_at(end).unwrap();
    // The window starts at the keyframe at or before end - 5 s: whole groups of pictures (a keyframe every two seconds).
    assert!(snapshot.video.samples[0].sync);
    let seconds = snapshot.duration_ms as f64 / 1000.0;
    assert!((5.0..=7.1).contains(&seconds), "window of {seconds} s");

    let dir = scratch("window");
    let id = "00000000-0000-4000-8000-000000000001";
    let path = dir.join(file_name("Test Pattern", id));
    let mut meta = ClipMetadata::new_manual(
        id.into(),
        1_700_000_000_000,
        "Test Pattern".into(),
        vec!["1".into()],
    );
    meta.thumbnail = "data:image/jpeg;base64,AAAA".into();
    let size = snapshot.write(&path, &mut meta).unwrap();
    assert_eq!(size, fs::metadata(&path).unwrap().len());
    assert_eq!(meta.length, snapshot.duration_ms);
    assert!(!path.with_extension("mp4.part").exists());

    let mut file = fs::File::open(&path).unwrap();
    let info = mp4::read_info(&mut file).unwrap();
    let video = info.video.clone().unwrap();
    assert_eq!(video.codec, *b"avc1");
    assert_eq!((video.width, video.height), (320, 180));
    assert_eq!(video.samples as usize, snapshot.video.samples.len());
    assert!(video.sync_samples >= 2);
    assert_eq!(
        info.duration_ms,
        snapshot
            .duration_ms
            .max(info.audio.iter().map(|a| a.duration_ms).max().unwrap_or(0))
    );
    // Two audio tracks, 20 ms packets, close to the video's length.
    let names: Vec<&str> = info.audio.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["application", "voice"]);
    for audio in &info.audio {
        assert_eq!(audio.codec, *b"Opus");
        let drift = audio.duration_ms as i64 - snapshot.duration_ms as i64;
        assert!(drift.abs() <= 300, "{} drifts {drift} ms", audio.name);
    }
    assert_eq!(info.audio[0].channels, 2);
    assert_eq!(info.audio[1].channels, 1);
    let stored = ClipMetadata::parse(&info.metadata.clone().unwrap()).unwrap();
    assert_eq!(stored, meta);
    assert_eq!(stored.length, snapshot.duration_ms);

    // The pictures decode: the first sample is a keyframe, and every sample gives a picture of the right size.
    let units = mp4::read_video_units(&mut file).unwrap();
    assert_eq!(units.len(), video.samples as usize);
    let mut decoder = Decoder::software().unwrap();
    let mut pictures = 0;
    for unit in &units {
        if let Ok(Some(picture)) = decoder.decode(unit) {
            assert_eq!((picture.width, picture.height), (320, 180));
            pictures += 1;
        }
    }
    assert!(
        pictures + 2 >= units.len(),
        "{pictures} of {} decoded",
        units.len()
    );

    // The library loads the directory back.
    let clips = library::load_directory(&dir);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].meta, meta);
    assert_eq!(clips[0].size_bytes, size);
    assert_eq!(clips[0].filepath, path);
    // For an outside check (ffprobe): keep a copy when asked to.
    if let Some(keep) = std::env::var_os("DC_CLIPS_KEEP") {
        fs::copy(&path, keep).unwrap();
    }
    fs::remove_dir_all(&dir).unwrap();
    recorder.configure(false, 5_000);
}

#[test]
fn the_buffer_never_holds_much_more_than_the_clip_length() {
    let recorder = standalone();
    feed(recorder, 5_000, 12);
    let (bytes, application, voice) = recorder.held();
    assert!(bytes > 0);
    // Audio is kept for the length plus the slack, 20 ms a packet.
    assert!(
        application <= (5_000 + 4_000) / 20 + 5,
        "{application} packets"
    );
    assert!(voice <= (5_000 + 4_000) / 20 + 5, "{voice} packets");
    recorder.configure(false, 5_000);
}

#[test]
fn a_short_stream_gives_a_short_clip_and_a_quiet_voice_is_silence_not_a_shorter_track() {
    let recorder = standalone();
    let end = feed(recorder, 30_000, 8);
    let snapshot = recorder.snapshot_at(end).unwrap();
    // Everything from the first keyframe: all eight seconds.
    assert!((7.8..=8.1).contains(&(snapshot.duration_ms as f64 / 1000.0)));
    let voice = snapshot
        .audio
        .iter()
        .find(|track| track.name == "voice")
        .unwrap();
    // The second without voice is filled, so the voice track is as long as the video (within a packet or two).
    let voice_ms = voice.packets.len() as u64 * 20;
    assert!(
        voice_ms.abs_diff(snapshot.duration_ms) <= 100,
        "{voice_ms} vs {}",
        snapshot.duration_ms
    );
    recorder.configure(false, 30_000);
}

#[test]
fn no_source_means_no_clip() {
    let recorder = standalone();
    recorder.configure(true, 30_000);
    assert_eq!(
        recorder.snapshot_at(10).err(),
        Some(SnapshotError::NoSource)
    );
}

/// Reads the clip files in `DC_CLIPS_CHECK_DIR` with this crate's loader and prints what they say: a way to check a clip
/// written by the app itself (for example from a harness run).
#[test]
#[ignore = "needs DC_CLIPS_CHECK_DIR"]
fn reads_the_clips_of_a_folder() {
    let dir = std::env::var_os("DC_CLIPS_CHECK_DIR").expect("DC_CLIPS_CHECK_DIR");
    let clips = library::load_directory(std::path::Path::new(&dir));
    assert!(!clips.is_empty());
    for clip in clips {
        let mut file = fs::File::open(&clip.filepath).unwrap();
        let info = mp4::read_info(&mut file).unwrap();
        let units = mp4::read_video_units(&mut file).unwrap();
        let video = info.video.unwrap();
        println!(
            "CHECK {} bytes, {} ms, video {}x{} {} samples ({} sync), audio {:?}, length in metadata {} ms, name {}",
            info.size,
            info.duration_ms,
            video.width,
            video.height,
            video.samples,
            video.sync_samples,
            info.audio
                .iter()
                .map(|a| (a.name.clone(), a.samples, a.channels))
                .collect::<Vec<_>>(),
            clip.meta.length,
            clip.filepath.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(units.len(), video.samples as usize);
        let mut decoder = Decoder::software().unwrap();
        let decoded = units
            .iter()
            .filter(|unit| matches!(decoder.decode(unit), Ok(Some(_))))
            .count();
        println!("CHECK decoded {decoded} of {} pictures", units.len());
    }
}

#[test]
fn a_clip_is_read_back_whole_trimmed_and_its_document_replaced() {
    let recorder = standalone();
    let end = feed(recorder, 8_000, 12);
    let snapshot = recorder.snapshot_at(end).unwrap();
    let dir = scratch("edit");
    let id = "00000000-0000-4000-8000-000000000002";
    let path = dir.join(file_name("Test Pattern", id));
    let mut meta = ClipMetadata::new_manual(id.into(), 1, "Test Pattern".into(), vec![]);
    snapshot.write(&path, &mut meta).unwrap();

    let whole = mp4::read_clip(&mut fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(whole.video.samples.len(), snapshot.video.samples.len());
    assert_eq!(whole.audio.len(), 2);
    assert_eq!(whole.audio[0].name, "application");
    assert_eq!(whole.audio[1].name, "voice");
    assert_eq!(whole.audio[0].channels, 2);
    assert_eq!(whole.audio[1].channels, 1);
    let (starts, length) = whole.video_starts_ms();
    assert_eq!(starts.len(), whole.video.samples.len());
    assert!(length.abs_diff(snapshot.duration_ms) <= 1);

    // A trim from 3 s to 6 s starts at a keyframe at or before 3 s and ends with the last picture before 6 s.
    let cut = whole.trimmed(3_000, 6_000).unwrap();
    assert!(cut.video.samples[0].sync);
    let (cut_starts, cut_length) = cut.video_starts_ms();
    assert!((3_000..=5_100).contains(&cut_length), "{cut_length}");
    assert_eq!(cut_starts[0], 0);
    let first = starts
        .iter()
        .enumerate()
        .rfind(|(i, s)| **s <= 3_000 && whole.video.samples[*i].sync)
        .unwrap()
        .0;
    assert_eq!(
        cut.video.samples.len(),
        starts.iter().rposition(|s| *s < 6_000).unwrap() - first + 1
    );
    assert!(
        cut.audio[0].packets.len() as u64 * 20 >= cut_length - 20
            && cut.audio[0].packets.len() as u64 * 20 <= cut_length + 40
    );

    // The trimmed copy is a clip file of its own: it reads back and decodes.
    let copy = dir.join("copy.mp4");
    let mut out = std::io::BufWriter::new(fs::File::create(&copy).unwrap());
    cut.write(&mut out, meta.to_json().as_bytes()).unwrap();
    drop(out);
    let units = mp4::read_video_units(&mut fs::File::open(&copy).unwrap()).unwrap();
    assert_eq!(units.len(), cut.video.samples.len());
    let mut decoder = Decoder::software().unwrap();
    let mut pictures = 0;
    for unit in &units {
        if decoder.decode(unit).unwrap().is_some() {
            pictures += 1;
        }
    }
    assert!(pictures > 0);

    // Switching an audio track off leaves the other one.
    let mut quiet = mp4::read_clip(&mut fs::File::open(&copy).unwrap()).unwrap();
    quiet.keep_audio(&["voice"]);
    assert_eq!(quiet.audio.len(), 1);
    assert_eq!(quiet.audio[0].name, "voice");

    // Replacing the document keeps the picture and sound and swaps the metadata.
    let before = fs::read(&path).unwrap();
    let mut renamed = meta.clone();
    renamed.name = Some("My best round".into());
    mp4::replace_metadata(&path, renamed.to_json().as_bytes()).unwrap();
    let loaded = library::load_file(&path).unwrap();
    assert_eq!(loaded.meta.name.as_deref(), Some("My best round"));
    let after = mp4::read_clip(&mut fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(after.video.samples.len(), whole.video.samples.len());
    assert_eq!(after.audio[0].packets, whole.audio[0].packets);
    // Everything before the document is byte for byte what it was.
    let kept = before.len() - (8 + 16 + meta.to_json().len());
    assert_eq!(&before[..kept], &fs::read(&path).unwrap()[..kept]);
    assert!(!path.with_extension("mp4.part").exists());
}
