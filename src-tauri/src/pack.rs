// Building a Shimeji-ee package from shimejis.xyz sprites.
//
// On shimejis.xyz the "Get it" button leads to /activate and requires a login: it
// is meant for their Chrome extension, and no ready-made zip is offered. But the
// sprites are separate files at predictable addresses, and the package format is
// simple and publicly documented (in their own FAQ):
//
//   Package.zip
//   ├── img/
//   │   └── {Name}/
//   │       ├── shime1.png
//   │       └── shime2.png
//   ├── actions.xml    (optional)
//   └── behaviors.xml  (optional)
//
// So we simply build such a zip ourselves and hand it to `shimejictl convert`.
//
// One catch: wl_shimeji rejects a package without actions.xml / behaviors.xml.
// shimejis.xyz ships only sprites, so we embed the default Shimeji-ee conf files
// in the binary and add them to every package we build (see DEFAULT_ACTIONS).

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

use crate::catalog;

/// Default Shimeji-ee conf files, embedded in the binary.
///
/// shimejis.xyz serves only sprites, and wl_shimeji rejects a package without
/// actions.xml / behaviors.xml:
///   "Failed to parse Shimeji-EE instance: Passed input is not a valid instance"
///
/// So we carry default files with us. Source: conf/ from Shimeji-ee
/// (github.com/TigerHix/shimeji-ee), New BSD license.
const DEFAULT_ACTIONS: &str = include_str!("../assets/default-actions.xml");
const DEFAULT_BEHAVIORS: &str = include_str!("../assets/default-behaviors.xml");

/// Contents of a conf file: the embedded Shimeji-ee default.
fn conf_content(name: &str) -> Vec<u8> {
    match name {
        "actions.xml" => DEFAULT_ACTIONS.as_bytes().to_vec(),
        _ => DEFAULT_BEHAVIORS.as_bytes().to_vec(),
    }
}

/// The error a cancelled install returns. Callers compare against it to tell
/// "the user pressed Cancel" from a real failure.
pub const CANCELLED: &str = "cancelled";

/// The actions of a package refer to frames 1 to `total`. The site does not have all of
/// them for every character (Lestrade lacked the first three), and the overlay goes down
/// when an action refers to a picture that is not there. So every gap gets a copy of the
/// nearest frame that exists: the character works, with fewer distinct poses.
fn fill_gaps(sprites: &mut Vec<(usize, Vec<u8>)>, total: usize) {
    if sprites.is_empty() {
        return;
    }
    let have: std::collections::BTreeMap<usize, Vec<u8>> = sprites.iter().cloned().collect();
    for n in 1..=total {
        if have.contains_key(&n) {
            continue;
        }
        let nearest = have.range(..n).next_back().or_else(|| have.range(n..).next());
        if let Some((_, bytes)) = nearest {
            sprites.push((n, bytes.clone()));
        }
    }
    sprites.sort_by_key(|(n, _)| *n);
}

/// Downloads a character's sprites while they exist and builds a zip from them.
/// Returns the path of the created archive.
///
/// `cancelled` is polled between download batches, so Cancel takes effect
/// within a fraction of a second instead of after the whole character.
pub async fn build_package(
    slug: &str,
    character_name: &str,
    dest_dir: &Path,
    // How many of the 46 frames are done, for the progress indicator.
    progress: impl Fn(usize) + Send + Sync,
    cancelled: impl Fn() -> bool + Send + Sync,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| format!("could not create the folder {}: {e}", dest_dir.display()))?;

    let client = catalog::client();
    let base = catalog::sprite_base(slug);

    // The classic Shimeji-ee set is 46 frames. We download in parallel batches of 6
    // and stop when several in a row are missing (some characters have fewer frames,
    // and numbering sometimes has gaps).
    const TOTAL: usize = 46;
    const CHUNK: usize = 6;

    let mut sprites: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut misses_in_row = 0;
    let mut next = 1usize;

    'download: while next <= TOTAL {
        if cancelled() {
            return Err(CANCELLED.to_string());
        }
        let batch: Vec<usize> = (next..=(next + CHUNK - 1).min(TOTAL)).collect();

        let results = futures::future::join_all(batch.iter().map(|&n| {
            let client = client.clone();
            let url = format!("{base}/shime{n}.png");
            async move {
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        resp.bytes().await.ok().filter(|b| !b.is_empty()).map(|b| b.to_vec())
                    }
                    _ => None,
                }
            }
        }))
        .await;

        // Process in order so the "5 missing in a row" rule works as before.
        for (n, got) in batch.iter().zip(results) {
            match got {
                Some(bytes) => {
                    sprites.push((*n, bytes));
                    misses_in_row = 0;
                }
                None => misses_in_row += 1,
            }
            if misses_in_row >= 5 && !sprites.is_empty() {
                break 'download;
            }
        }

        next += CHUNK;
        progress((next - 1).min(TOTAL));
    }
    progress(TOTAL);

    if sprites.is_empty() {
        return Err(format!(
            "could not download any sprite for \"{character_name}\""
        ));
    }
    fill_gaps(&mut sprites, TOTAL);

    // The folder name inside the archive must be safe for the file system.
    let folder: String = character_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let folder = if folder.trim_matches('_').is_empty() {
        slug.to_string()
    } else {
        folder
    };

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for (n, bytes) in &sprites {
            zip.start_file(format!("img/{folder}/shime{n}.png"), opts)
                .map_err(|e| format!("could not write to the archive: {e}"))?;
            zip.write_all(bytes)
                .map_err(|e| format!("could not write the sprite: {e}"))?;
        }

        // Add the default conf files. Different builds look for them in different
        // places, so we put copies in all three known locations.
        for name in ["actions.xml", "behaviors.xml"] {
            let content = conf_content(name);
            for target in [
                format!("conf/{name}"),
                name.to_string(),
                format!("img/{folder}/conf/{name}"),
            ] {
                zip.start_file(&target, opts)
                    .map_err(|e| format!("could not write {target}: {e}"))?;
                zip.write_all(&content)
                    .map_err(|e| format!("could not write {target}: {e}"))?;
            }
        }

        zip.finish()
            .map_err(|e| format!("could not finalize the archive: {e}"))?;
    }

    let dest = dest_dir.join(format!("{folder}.zip"));
    std::fs::write(&dest, buf.into_inner())
        .map_err(|e| format!("could not save the archive {}: {e}", dest.display()))?;

    Ok(dest)
}

// ---------------------------------------------------------------------------
// Extracting a sprite from an already installed prototype
// ---------------------------------------------------------------------------

/// Extracts a character's first image from its local data.
///
/// Needed for characters installed before this app existed: the shimejis.xyz
/// catalog may not have them at all (for example Hollow Knight), so matching by
/// name fails and the cards stay empty.
///
/// We try two sources:
///   1. unpacked folders in prototypes/ and shimejis/, looking for shime1
///   2. `prototypes export` and an attempt to read the .wlshm as a zip
///
/// wl_shimeji stores frames in `assets/` in QOI format (`shime1.qoi`), so before
/// saving we re-encode them as PNG: the UI only shows PNG.
pub fn extract_local_sprite(name: &str, dest: &Path) -> Result<(), String> {
    // --- 1. folders on disk ---
    for dir in crate::shimejictl::prototype_dirs() {
        let candidates = [dir.join(name), dir.join(format!("Shimeji.{name}"))];
        for base in candidates {
            if !base.is_dir() {
                continue;
            }
            if let Some(img) = find_first_sprite(&base, 4) {
                let bytes = std::fs::read(&img)
                    .map_err(|e| format!("could not read {}: {e}", img.display()))?;
                return save_sprite(&img.to_string_lossy(), &bytes, dest);
            }
        }
    }

    // --- 2. export and try to read it as a zip ---
    let tmp = crate::system::scratch_dir().join("sprite-probe");
    std::fs::create_dir_all(&tmp).ok();
    let file = tmp.join("probe.wlshm");
    let _ = std::fs::remove_file(&file);

    crate::shimejictl::export_prototype(name, &file)?;

    let f = std::fs::File::open(&file).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(f)
        .map_err(|e| format!(".wlshm is not a zip archive: {e}"))?;

    // Look for shime1.{png,qoi}, otherwise any image.
    let mut best: Option<usize> = None;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let n = entry.name().to_lowercase();
        if n.ends_with("shime1.png") || n.ends_with("shime1.qoi") {
            best = Some(i);
            break;
        }
        if best.is_none() && is_sprite_name(&n) {
            best = Some(i);
        }
    }

    let idx = best.ok_or("the prototype has no images")?;
    let mut entry = zip.by_index(idx).map_err(|e| e.to_string())?;
    let entry_name = entry.name().to_string();
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    save_sprite(&entry_name, &buf, dest)
}

fn is_sprite_name(lower: &str) -> bool {
    lower.ends_with(".png") || lower.ends_with(".qoi")
}

/// Saves an image as PNG: PNG is copied as is, QOI is re-encoded.
fn save_sprite(source_name: &str, bytes: &[u8], dest: &Path) -> Result<(), String> {
    if source_name.to_lowercase().ends_with(".qoi") {
        let (w, h, rgba) = decode_qoi(bytes)?;
        write_png(dest, w, h, &rgba)
    } else {
        std::fs::write(dest, bytes).map_err(|e| format!("could not save the sprite: {e}"))
    }
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| format!("PNG: {e}"))?;
        writer.write_image_data(rgba).map_err(|e| format!("PNG: {e}"))?;
    }
    Ok(out)
}

fn write_png(dest: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let bytes = encode_png(w, h, rgba)?;
    std::fs::write(dest, bytes).map_err(|e| format!("could not create {}: {e}", dest.display()))
}

/// Standard base64 (RFC 4648), so frames can travel to the UI as `data:` URLs
/// without an extra dependency.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if c.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// The numbered animation frames (`shime1`, `shime2`, ...) of an installed
/// prototype as PNG `data:` URLs, in order, at most `max`. Used for the hover
/// preview, so it only reads the frames on disk (no shimejictl call) and returns
/// an empty list when the prototype is not an unpacked folder.
///
/// Lettered variants (`shime11a`) are skipped: they belong to special actions,
/// and the plain numbered run is what the catalog preview shows too.
pub fn local_frames(name: &str, max: usize) -> Vec<String> {
    for dir in crate::shimejictl::prototype_dirs() {
        for base in [dir.join(name), dir.join(format!("Shimeji.{name}"))] {
            let assets = base.join("assets");
            let Ok(entries) = std::fs::read_dir(&assets) else { continue };

            let mut numbered: Vec<(u32, PathBuf)> = entries
                .flatten()
                .filter_map(|e| {
                    let path = e.path();
                    let file = path.file_name()?.to_string_lossy().to_lowercase();
                    let stem = file.strip_suffix(".qoi").or_else(|| file.strip_suffix(".png"))?;
                    let n: u32 = stem.strip_prefix("shime")?.parse().ok()?;
                    Some((n, path))
                })
                .collect();
            numbered.sort_by_key(|(n, _)| *n);
            numbered.dedup_by_key(|(n, _)| *n);

            let frames: Vec<String> = numbered
                .into_iter()
                .take(max)
                .filter_map(|(_, path)| {
                    let bytes = std::fs::read(&path).ok()?;
                    let png = if path.extension().map(|x| x == "qoi").unwrap_or(false) {
                        let (w, h, rgba) = decode_qoi(&bytes).ok()?;
                        encode_png(w, h, &rgba).ok()?
                    } else {
                        bytes
                    };
                    Some(format!("data:image/png;base64,{}", base64(&png)))
                })
                .collect();
            if !frames.is_empty() {
                return frames;
            }
        }
    }
    Vec::new()
}

/// QOI decoder (https://qoiformat.org/qoi-specification.pdf) → (width,
/// height, RGBA). The format is simple, so no separate dependency is needed.
fn decode_qoi(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    const HEADER: usize = 14;
    const END_MARKER: usize = 8;

    if data.len() < HEADER + END_MARKER || &data[0..4] != b"qoif" {
        return Err("not a QOI image".into());
    }
    let w = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let h = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let total = (w as usize)
        .checked_mul(h as usize)
        .filter(|t| *t > 0 && *t <= 16_384 * 16_384)
        .ok_or("QOI: invalid dimensions")?;

    let hash = |p: [u8; 4]| {
        (p[0] as usize * 3 + p[1] as usize * 5 + p[2] as usize * 7 + p[3] as usize * 11) % 64
    };
    let truncated = || "QOI: file is truncated".to_string();

    let mut out = Vec::with_capacity(total * 4);
    let mut index = [[0u8; 4]; 64];
    let mut px = [0u8, 0, 0, 255];
    let mut run = 0u8;
    let mut p = HEADER;
    let end = data.len() - END_MARKER;

    for _ in 0..total {
        if run > 0 {
            run -= 1;
        } else {
            if p >= end {
                return Err(truncated());
            }
            let b1 = data[p];
            p += 1;

            match b1 {
                0xFE => {
                    let c = data.get(p..p + 3).ok_or_else(truncated)?;
                    px[..3].copy_from_slice(c);
                    p += 3;
                }
                0xFF => {
                    let c = data.get(p..p + 4).ok_or_else(truncated)?;
                    px.copy_from_slice(c);
                    p += 4;
                }
                _ => match b1 & 0xC0 {
                    0x00 => px = index[(b1 & 0x3F) as usize],
                    0x40 => {
                        px[0] = px[0].wrapping_add((b1 >> 4) & 3).wrapping_sub(2);
                        px[1] = px[1].wrapping_add((b1 >> 2) & 3).wrapping_sub(2);
                        px[2] = px[2].wrapping_add(b1 & 3).wrapping_sub(2);
                    }
                    0x80 => {
                        let b2 = *data.get(p).ok_or_else(truncated)?;
                        p += 1;
                        let dg = (b1 & 0x3F).wrapping_sub(32);
                        px[0] = px[0].wrapping_add(dg).wrapping_sub(8).wrapping_add(b2 >> 4);
                        px[1] = px[1].wrapping_add(dg);
                        px[2] = px[2].wrapping_add(dg).wrapping_sub(8).wrapping_add(b2 & 0x0F);
                    }
                    _ => run = b1 & 0x3F,
                },
            }
            index[hash(px)] = px;
        }
        out.extend_from_slice(&px);
    }

    Ok((w, h, out))
}

fn find_first_sprite(dir: &Path, depth: usize) -> Option<std::path::PathBuf> {
    if depth == 0 {
        return None;
    }

    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    let mut fallback = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        let n = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        if n == "shime1.png" || n == "shime1.qoi" {
            return Some(path);
        }
        if fallback.is_none() && is_sprite_name(&n) {
            fallback = Some(path);
        }
    }

    for sub in subdirs {
        if let Some(found) = find_first_sprite(&sub, depth - 1) {
            return Some(found);
        }
    }

    fallback
}

#[cfg(test)]
mod qoi_tests {
    use super::*;

    /// A frame of a real prototype decodes to 128×128 with non-transparent
    /// pixels. Skipped if Hornet is not installed.
    #[test]
    fn decodes_installed_sprite() {
        let path = crate::shimejictl::config_root().join("shimejis/Shimeji.Hornet/assets/shime1.qoi");
        let Ok(bytes) = std::fs::read(&path) else { return };
        let (w, h, rgba) = decode_qoi(&bytes).unwrap();
        assert_eq!((w, h), (128, 128));
        assert_eq!(rgba.len(), 128 * 128 * 4);
        assert!(rgba.chunks(4).any(|p| p[3] > 0), "every pixel is transparent");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_qoi(b"not a qoi file at all....").is_err());
    }
}

#[cfg(test)]
mod local_sprite_tests {
    use super::*;

    /// A frame of an installed prototype (QOI) turns into a real PNG.
    #[test]
    fn extracts_installed_prototype_as_png() {
        let dir = crate::shimejictl::config_root().join("shimejis/Shimeji.Hornet");
        if !dir.is_dir() {
            return;
        }
        let dest = std::env::temp_dir().join(format!("menagerie-test-{}.png", std::process::id()));
        extract_local_sprite("Hornet", &dest).expect("extract");

        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&dest).unwrap()));
        let reader = dec.read_info().expect("a real PNG");
        assert_eq!((reader.info().width, reader.info().height), (128, 128));
        let _ = std::fs::remove_file(dest);
    }
}

#[cfg(test)]
mod gap_tests {
    use super::fill_gaps;

    #[test]
    fn a_package_always_has_every_frame_the_actions_refer_to() {
        // The site had 4, 5 and 9 only.
        let mut got = vec![(4, vec![4u8]), (5, vec![5]), (9, vec![9])];
        fill_gaps(&mut got, 12);
        let numbers: Vec<usize> = got.iter().map(|(n, _)| *n).collect();
        assert_eq!(numbers, (1..=12).collect::<Vec<_>>());
        let bytes = |n: usize| got.iter().find(|(k, _)| *k == n).unwrap().1.clone();
        assert_eq!(bytes(1), vec![4], "before the first: the first");
        assert_eq!(bytes(4), vec![4], "real frames are kept");
        assert_eq!(bytes(7), vec![5], "a gap: the nearest one below");
        assert_eq!(bytes(12), vec![9]);
    }

    #[test]
    fn nothing_downloaded_stays_nothing() {
        let mut none: Vec<(usize, Vec<u8>)> = Vec::new();
        fill_gaps(&mut none, 46);
        assert!(none.is_empty());
    }
}

#[cfg(test)]
mod frames_tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_test_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn png_encoding_roundtrips_through_the_decoder() {
        let rgba: Vec<u8> = (0..4 * 4 * 4).map(|i| (i * 7) as u8).collect();
        let png = encode_png(4, 4, &rgba).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (4, 4));
        assert_eq!(&buf[..64], &rgba[..]);
    }

    /// Frames of a real installed prototype: several, in order, all valid PNGs.
    /// Skipped when nothing suitable is installed.
    #[test]
    fn installed_prototype_yields_an_ordered_run_of_png_frames() {
        let Some(name) = ["Hornet", "Zooble", "BMO"].into_iter().find(|n| !local_frames(n, 3).is_empty()) else { return };
        let frames = local_frames(name, 46);
        eprintln!("{name}: {} frames, first {} bytes", frames.len(), frames[0].len());
        assert!(frames.len() >= 2 && frames.len() <= 46, "{} frames for {name}", frames.len());
        assert!(frames.iter().all(|f| f.starts_with("data:image/png;base64,")));
        assert_ne!(frames[0], frames[1], "frames should differ");
    }
}

#[cfg(test)]
mod cancel_tests {
    use super::*;

    /// Cancel is checked before the first batch, so a flag that is already set
    /// stops the install without touching the network.
    #[test]
    fn a_cancelled_install_stops_before_any_download() {
        let dir = std::env::temp_dir().join(format!("menagerie-cancel-{}", std::process::id()));
        let result = tauri::async_runtime::block_on(build_package("any-slug", "Any", &dir, |_| {}, || true));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(result.unwrap_err(), CANCELLED);
    }
}
