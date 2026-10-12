//! EXIF reading for photos.

use std::io::BufReader;
use std::path::Path;

use ::exif::{DateTime, Exif, Field, In, Reader, Tag, Value};
use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExifInfo {
    /// DateTimeOriginal (or DateTime) as unix seconds, interpreted with OffsetTimeOriginal when present, else UTC.
    pub taken_at: Option<i64>,
    /// "Make Model", e.g. "Apple iPhone 15 Pro".
    pub camera: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// EXIF orientation 1–8.
    pub orientation: Option<u16>,
}

impl ExifInfo {
    /// True when the orientation turns the image sideways (5–8), so width and height swap on screen.
    pub fn swaps_dimensions(&self) -> bool {
        matches!(self.orientation, Some(5..=8))
    }

    /// Width and height as displayed, after applying the orientation.
    pub fn display_size(&self) -> (Option<i64>, Option<i64>) {
        if self.swaps_dimensions() { (self.height, self.width) } else { (self.width, self.height) }
    }

    fn is_empty(&self) -> bool {
        self == &ExifInfo::default()
    }
}

/// Blocking EXIF read (JPEG, HEIC, TIFF, PNG, WebP, DNG). Ok(None) when absent.
///
/// Corrupt or unsupported files yield Ok(None); only I/O errors opening the file are errors.
pub fn read(path: &Path) -> Result<Option<ExifInfo>> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    // The parser is defensive, but a malformed file must never take a worker down.
    let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Reader::new().read_from_container(&mut reader)
    }));
    let exif = match parsed {
        Ok(Ok(exif)) => exif,
        Ok(Err(::exif::Error::PartialResult(partial))) => partial.into_inner().0,
        Ok(Err(::exif::Error::Io(err))) if err.kind() != std::io::ErrorKind::UnexpectedEof => {
            tracing::debug!(path = %path.display(), error = %err, "EXIF read failed");
            return Ok(None);
        }
        Ok(Err(err)) => {
            tracing::trace!(path = %path.display(), error = %err, "no EXIF");
            return Ok(None);
        }
        Err(_) => {
            tracing::warn!(path = %path.display(), "EXIF parser panicked; ignoring EXIF for this file");
            return Ok(None);
        }
    };
    Ok(from_exif(&exif))
}

/// Parse EXIF from a raw TIFF-structured buffer (the payload of a JPEG APP1 segment).
pub fn read_raw(buf: Vec<u8>) -> Option<ExifInfo> {
    let parsed = std::panic::catch_unwind(move || Reader::new().read_raw(buf));
    match parsed {
        Ok(Ok(exif)) => from_exif(&exif),
        Ok(Err(::exif::Error::PartialResult(partial))) => from_exif(&partial.into_inner().0),
        _ => None,
    }
}

fn from_exif(exif: &Exif) -> Option<ExifInfo> {
    let field = |tag: Tag| exif.get_field(tag, In::PRIMARY);
    let taken_at = [
        (Tag::DateTimeOriginal, Tag::OffsetTimeOriginal),
        (Tag::DateTimeDigitized, Tag::OffsetTimeDigitized),
        (Tag::DateTime, Tag::OffsetTime),
    ]
    .into_iter()
    .find_map(|(time_tag, offset_tag)| datetime(field(time_tag)?, field(offset_tag)));

    let camera = camera_name(field(Tag::Make).and_then(ascii).as_deref(), field(Tag::Model).and_then(ascii).as_deref());
    let width = field(Tag::PixelXDimension)
        .or_else(|| field(Tag::ImageWidth))
        .and_then(|f| f.value.get_uint(0))
        .filter(|v| *v > 0)
        .map(i64::from);
    let height = field(Tag::PixelYDimension)
        .or_else(|| field(Tag::ImageLength))
        .and_then(|f| f.value.get_uint(0))
        .filter(|v| *v > 0)
        .map(i64::from);
    let orientation = field(Tag::Orientation)
        .and_then(|f| f.value.get_uint(0))
        .and_then(|v| u16::try_from(v).ok())
        .filter(|v| (1..=8).contains(v));

    let info = ExifInfo { taken_at, camera, width, height, orientation };
    (!info.is_empty()).then_some(info)
}

/// First ASCII string of a field, trimmed of NULs and spaces.
fn ascii(field: &Field) -> Option<String> {
    match &field.value {
        Value::Ascii(parts) => parts
            .iter()
            .map(|p| String::from_utf8_lossy(p).trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string())
            .find(|s| !s.is_empty()),
        _ => None,
    }
}

fn first_bytes(field: &Field) -> Option<&[u8]> {
    match &field.value {
        Value::Ascii(parts) => parts.first().map(Vec::as_slice),
        _ => None,
    }
}

fn datetime(time_field: &Field, offset_field: Option<&Field>) -> Option<i64> {
    let mut dt = DateTime::from_ascii(first_bytes(time_field)?).ok()?;
    if let Some(offset) = offset_field.and_then(first_bytes) {
        // A malformed offset leaves the time as UTC.
        let _ = dt.parse_offset(offset);
    }
    to_unix(
        i32::from(dt.year),
        dt.month,
        dt.day,
        dt.hour,
        dt.minute,
        dt.second,
        i32::from(dt.offset.unwrap_or(0)),
    )
}

/// Calendar fields plus a UTC offset in minutes → unix seconds. None for impossible dates
/// and for the zeroed placeholders cameras write when the clock was never set.
pub(crate) fn to_unix(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8, offset_minutes: i32) -> Option<i64> {
    if !(1800..=2200).contains(&year) {
        return None;
    }
    let month = time::Month::try_from(month).ok()?;
    let date = time::Date::from_calendar_date(year, month, day).ok()?;
    let clock = time::Time::from_hms(hour, minute, second.min(59)).ok()?;
    let offset = time::UtcOffset::from_whole_seconds(offset_minutes.checked_mul(60)?).ok()?;
    let ts = time::PrimitiveDateTime::new(date, clock).assume_offset(offset).unix_timestamp();
    // 1970-01-01 (unix epoch) and 1904-01-01 (QuickTime epoch) mean "clock never set".
    (ts != 0 && ts != QUICKTIME_EPOCH).then_some(ts)
}

/// 1904-01-01T00:00:00Z, the zero point of QuickTime timestamps.
const QUICKTIME_EPOCH: i64 = -2_082_844_800;

/// "Apple" + "iPhone 15 Pro" → "Apple iPhone 15 Pro"; "Canon" + "Canon EOS R5" → "Canon EOS R5";
/// "NIKON CORPORATION" + "NIKON D850" → "NIKON D850".
pub fn camera_name(make: Option<&str>, model: Option<&str>) -> Option<String> {
    let clean = |s: Option<&str>| {
        s.map(|v| v.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string())
            .filter(|v| !v.is_empty())
    };
    match (clean(make), clean(model)) {
        (Some(make), Some(model)) => {
            let make_word = make.split_whitespace().next().unwrap_or(&make).to_lowercase();
            if model.to_lowercase().starts_with(&make_word) {
                Some(model)
            } else {
                Some(format!("{make} {model}"))
            }
        }
        (None, Some(model)) => Some(model),
        (Some(make), None) => Some(make),
        (None, None) => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Cursor;

    use ::exif::experimental::Writer;

    use super::*;

    pub(crate) struct TestExif<'a> {
        pub datetime_original: Option<&'a str>,
        pub offset_original: Option<&'a str>,
        pub datetime: Option<&'a str>,
        pub make: Option<&'a str>,
        pub model: Option<&'a str>,
        pub width: Option<u32>,
        pub height: Option<u32>,
        pub orientation: Option<u16>,
    }

    impl Default for TestExif<'_> {
        fn default() -> Self {
            Self {
                datetime_original: Some("2024:06:13 12:21:00"),
                offset_original: Some("+02:00"),
                datetime: None,
                make: Some("Apple"),
                model: Some("iPhone 15 Pro"),
                width: Some(4032),
                height: Some(3024),
                orientation: Some(6),
            }
        }
    }

    fn ascii_field(tag: Tag, value: &str) -> Field {
        Field { tag, ifd_num: In::PRIMARY, value: Value::Ascii(vec![value.as_bytes().to_vec()]) }
    }

    /// A TIFF-structured EXIF block, as found after `Exif\0\0` in a JPEG APP1 segment.
    pub(crate) fn tiff_block(spec: &TestExif<'_>) -> Vec<u8> {
        let mut fields = Vec::new();
        if let Some(v) = spec.datetime_original {
            fields.push(ascii_field(Tag::DateTimeOriginal, v));
        }
        if let Some(v) = spec.offset_original {
            fields.push(ascii_field(Tag::OffsetTimeOriginal, v));
        }
        if let Some(v) = spec.datetime {
            fields.push(ascii_field(Tag::DateTime, v));
        }
        if let Some(v) = spec.make {
            fields.push(ascii_field(Tag::Make, v));
        }
        if let Some(v) = spec.model {
            fields.push(ascii_field(Tag::Model, v));
        }
        if let Some(v) = spec.width {
            fields.push(Field { tag: Tag::PixelXDimension, ifd_num: In::PRIMARY, value: Value::Long(vec![v]) });
        }
        if let Some(v) = spec.height {
            fields.push(Field { tag: Tag::PixelYDimension, ifd_num: In::PRIMARY, value: Value::Long(vec![v]) });
        }
        if let Some(v) = spec.orientation {
            fields.push(Field { tag: Tag::Orientation, ifd_num: In::PRIMARY, value: Value::Short(vec![v]) });
        }
        let mut writer = Writer::new();
        for f in &fields {
            writer.push_field(f);
        }
        let mut buf = Cursor::new(Vec::new());
        writer.write(&mut buf, false).unwrap();
        buf.into_inner()
    }

    /// Insert an APP1 EXIF segment right after the SOI marker of a JPEG.
    pub(crate) fn inject_into_jpeg(jpeg: &[u8], tiff: &[u8]) -> Vec<u8> {
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "not a JPEG");
        let len = u16::try_from(tiff.len() + 8).unwrap();
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(b"Exif\0\0");
        out.extend_from_slice(tiff);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    pub(crate) fn make_jpeg(path: &Path, size: &str) {
        let status = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("testsrc=s={size}:d=1"))
            .args(["-frames:v", "1"])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn jpeg_with_offset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("IMG_0001.jpg");
        make_jpeg(&path, "320x240");
        let jpeg = std::fs::read(&path).unwrap();
        std::fs::write(&path, inject_into_jpeg(&jpeg, &tiff_block(&TestExif::default()))).unwrap();
        let info = read(&path).unwrap().unwrap();
        // 12:21 at +02:00 is 10:21 UTC.
        assert_eq!(info.taken_at, Some(1_718_274_060));
        assert_eq!(info.camera.as_deref(), Some("Apple iPhone 15 Pro"));
        assert_eq!((info.width, info.height), (Some(4032), Some(3024)));
        assert_eq!(info.orientation, Some(6));
        assert!(info.swaps_dimensions());
        assert_eq!(info.display_size(), (Some(3024), Some(4032)));
    }

    #[test]
    fn no_offset_means_utc_and_datetime_fallback() {
        let spec = TestExif {
            datetime_original: None,
            offset_original: None,
            datetime: Some("2024:06:13 10:21:00"),
            make: Some("Canon"),
            model: Some("Canon EOS R5"),
            orientation: Some(1),
            ..TestExif::default()
        };
        let info = read_raw(tiff_block(&spec)).unwrap();
        assert_eq!(info.taken_at, Some(1_718_274_060));
        assert_eq!(info.camera.as_deref(), Some("Canon EOS R5"));
        assert!(!info.swaps_dimensions());

        let spec = TestExif { offset_original: None, ..TestExif::default() };
        assert_eq!(read_raw(tiff_block(&spec)).unwrap().taken_at, Some(1_718_281_260));
    }

    #[test]
    fn placeholder_dates_are_ignored() {
        let spec = TestExif { datetime_original: Some("0000:00:00 00:00:00"), ..TestExif::default() };
        assert_eq!(read_raw(tiff_block(&spec)).unwrap().taken_at, None);
        let spec = TestExif { datetime_original: Some("    :  :     :  :  "), ..TestExif::default() };
        assert_eq!(read_raw(tiff_block(&spec)).unwrap().taken_at, None);
        let spec = TestExif { datetime_original: Some("2024:02:30 10:00:00"), ..TestExif::default() };
        assert_eq!(read_raw(tiff_block(&spec)).unwrap().taken_at, None);
        // Bad offset: keep the time as UTC.
        let spec = TestExif { offset_original: Some("junk"), ..TestExif::default() };
        assert_eq!(read_raw(tiff_block(&spec)).unwrap().taken_at, Some(1_718_281_260));
    }

    #[test]
    fn tiff_file_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.tif");
        std::fs::write(&path, tiff_block(&TestExif { orientation: Some(8), ..TestExif::default() })).unwrap();
        let info = read(&path).unwrap().unwrap();
        assert_eq!(info.orientation, Some(8));
        assert_eq!(info.camera.as_deref(), Some("Apple iPhone 15 Pro"));
    }

    #[test]
    fn absent_and_corrupt_files() {
        let dir = tempfile::tempdir().unwrap();
        // A JPEG without EXIF.
        let plain = dir.path().join("plain.jpg");
        make_jpeg(&plain, "64x64");
        assert_eq!(read(&plain).unwrap(), None);
        // PNG without EXIF.
        let png = dir.path().join("plain.png");
        make_jpeg(&png, "64x64");
        assert_eq!(read(&png).unwrap(), None);
        // Random bytes, empty file, truncated JPEG, a JPEG with a broken EXIF block.
        let junk = dir.path().join("junk.jpg");
        std::fs::write(&junk, b"\x00\x01\x02 definitely not an image").unwrap();
        assert_eq!(read(&junk).unwrap(), None);
        let empty = dir.path().join("empty.heic");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(read(&empty).unwrap(), None);
        let jpeg = std::fs::read(&plain).unwrap();
        let with_exif = inject_into_jpeg(&jpeg, &tiff_block(&TestExif::default()));
        let truncated = dir.path().join("truncated.jpg");
        std::fs::write(&truncated, &with_exif[..40]).unwrap();
        assert_eq!(read(&truncated).unwrap(), None);
        let mut broken_tiff = tiff_block(&TestExif::default());
        for b in broken_tiff.iter_mut().skip(8).step_by(3) {
            *b = 0xFF;
        }
        let broken = dir.path().join("broken.jpg");
        std::fs::write(&broken, inject_into_jpeg(&jpeg, &broken_tiff)).unwrap();
        let _ = read(&broken).unwrap();
        // Missing file is an I/O error, not a panic.
        assert!(read(&dir.path().join("missing.jpg")).is_err());
    }

    #[test]
    fn fuzzed_buffers_never_panic() {
        let base = tiff_block(&TestExif::default());
        let mut seed: u32 = 0x1234_5678;
        for _ in 0..300 {
            let mut buf = base.clone();
            for _ in 0..8 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let i = (seed as usize) % buf.len();
                buf[i] = (seed >> 24) as u8;
            }
            let _ = read_raw(buf);
        }
    }

    #[test]
    fn exiftool_written_jpeg() {
        let Ok(out) = std::process::Command::new("exiftool").arg("-ver").output() else {
            eprintln!("exiftool not installed; skipping");
            return;
        };
        if !out.status.success() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("DSC_0001.jpg");
        make_jpeg(&path, "200x100");
        let status = std::process::Command::new("exiftool")
            .args(["-q", "-overwrite_original", "-n", "-Orientation=8", "-Make=NIKON CORPORATION", "-Model=NIKON D850"])
            .args(["-DateTimeOriginal=2023:12:31 23:59:59", "-OffsetTimeOriginal=-05:00"])
            .args(["-ExifImageWidth=200", "-ExifImageHeight=100", "-GPSLatitude=37.7"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let info = read(&path).unwrap().unwrap();
        assert_eq!(info.camera.as_deref(), Some("NIKON D850"));
        assert_eq!(info.orientation, Some(8));
        // 23:59:59 at -05:00 is 04:59:59 UTC on Jan 1st.
        assert_eq!(info.taken_at, Some(1_704_085_199));
        assert_eq!(info.display_size(), (Some(100), Some(200)));
    }

    #[test]
    fn camera_names() {
        assert_eq!(camera_name(Some("Apple"), Some("iPhone 15 Pro")).as_deref(), Some("Apple iPhone 15 Pro"));
        assert_eq!(camera_name(Some("Canon"), Some("Canon EOS R5")).as_deref(), Some("Canon EOS R5"));
        assert_eq!(camera_name(Some("NIKON CORPORATION"), Some("NIKON D850")).as_deref(), Some("NIKON D850"));
        assert_eq!(camera_name(Some("samsung"), Some("SM-G991B")).as_deref(), Some("samsung SM-G991B"));
        assert_eq!(camera_name(Some("  "), Some("Pixel 8\0")).as_deref(), Some("Pixel 8"));
        assert_eq!(camera_name(Some("GoPro"), None).as_deref(), Some("GoPro"));
        assert_eq!(camera_name(None, None), None);
    }

    #[test]
    fn unix_conversion() {
        assert_eq!(to_unix(1970, 1, 1, 0, 0, 0, 0), None);
        assert_eq!(to_unix(2024, 6, 13, 10, 21, 0, 0), Some(1_718_274_060));
        assert_eq!(to_unix(2024, 6, 13, 12, 21, 0, 120), Some(1_718_274_060));
        assert_eq!(to_unix(2024, 13, 1, 0, 0, 0, 0), None);
        assert_eq!(to_unix(0, 0, 0, 0, 0, 0, 0), None);
    }
}
