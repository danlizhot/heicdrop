//! Logique de conversion, indépendante de l'interface graphique.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::tiff::TiffEncoder;
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, RgbImage};
use rayon::prelude::*;

pub const JPEG_QUALITY: u8 = 92;

/// Extensions reconnues en entrée.
pub const INPUT_EXTENSIONS: &[&str] = &[
    "heic", "heif", "hif", "avif", "jpg", "jpeg", "jpe", "jfif", "png", "webp", "bmp", "dib",
    "gif", "tif", "tiff", "ico", "tga", "ppm", "pgm", "pbm", "pnm", "qoi", "dds", "hdr",
];

const HEIF_EXTENSIONS: &[&str] = &["heic", "heif", "hif", "avif"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Jpeg,
    Png,
    Pdf,
    Webp,
    Tiff,
    Bmp,
    Gif,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 7] = [
        Self::Jpeg,
        Self::Png,
        Self::Pdf,
        Self::Webp,
        Self::Tiff,
        Self::Bmp,
        Self::Gif,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Pdf => "PDF",
            Self::Webp => "WEBP",
            Self::Tiff => "TIFF",
            Self::Bmp => "BMP",
            Self::Gif => "GIF",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Pdf => "pdf",
            Self::Webp => "webp",
            Self::Tiff => "tif",
            Self::Bmp => "bmp",
            Self::Gif => "gif",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.label() == s)
    }
}

/// Image chargée en mémoire, avec ses métadonnées.
pub struct Loaded {
    pub image: DynamicImage,
    pub exif: Option<Vec<u8>>,
    pub icc: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ConvResult {
    pub source: PathBuf,
    pub output: Option<PathBuf>,
    pub error: Option<String>,
}

impl ConvResult {
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }
}

// ---------------------------------------------------------------------------
// Collecte des fichiers
// ---------------------------------------------------------------------------

fn ext_lower(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn is_supported(p: &Path) -> bool {
    p.is_file() && INPUT_EXTENSIONS.contains(&ext_lower(p).as_str())
}

/// Développe une liste de fichiers et de dossiers (récursivement) en liste d'images.
pub fn collect_images<I, P>(paths: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for p in paths {
        walk(p.as_ref(), &mut out, &mut seen);
    }
    out
}

fn walk(p: &Path, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    if p.is_dir() {
        let mut entries: Vec<PathBuf> = match fs::read_dir(p) {
            Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
            Err(_) => return,
        };
        entries.sort();
        for e in entries {
            walk(&e, out, seen);
        }
    } else if is_supported(p) {
        let key = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        if seen.insert(key) {
            out.push(p.to_path_buf());
        }
    }
}

// ---------------------------------------------------------------------------
// Lecture
// ---------------------------------------------------------------------------

fn looks_like_heif(path: &Path) -> bool {
    if HEIF_EXTENSIONS.contains(&ext_lower(path).as_str()) {
        return true;
    }
    // Détection par signature : boîte ISO-BMFF "ftyp" + marque HEIF
    let Ok(bytes) = fs::read(path).map(|b| b.into_iter().take(16).collect::<Vec<u8>>()) else {
        return false;
    };
    bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(
            &bytes[8..12],
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"mif1" | b"msf1" | b"avif"
        )
}

pub fn load(path: &Path) -> Result<Loaded, String> {
    if looks_like_heif(path) {
        load_heif(path)
    } else {
        load_generic(path)
    }
}

fn load_heif(path: &Path) -> Result<Loaded, String> {
    use libheif_rs::{ColorSpace, DecodingOptions, HeifContext, ItemId, LibHeif, RgbChroma};

    let lib = LibHeif::new();
    let ctx = HeifContext::read_from_file(&path.to_string_lossy())
        .map_err(|e| format!("lecture HEIF impossible : {e}"))?;
    let handle = ctx
        .primary_image_handle()
        .map_err(|e| format!("image principale introuvable : {e}"))?;

    let has_alpha = handle.has_alpha_channel();
    let chroma = if has_alpha { RgbChroma::Rgba } else { RgbChroma::Rgb };
    let mut opts = DecodingOptions::new();
    if let Some(o) = opts.as_mut() {
        o.set_convert_hdr_to_8bit(true);
    }
    // libheif applique lui-même les rotations/miroirs (irot/imir).
    let img = lib
        .decode(&handle, ColorSpace::Rgb(chroma), opts)
        .map_err(|e| format!("décodage HEIF impossible : {e}"))?;

    let planes = img.planes();
    let plane = planes.interleaved.ok_or("format de pixels inattendu")?;
    let (w, h) = (plane.width, plane.height);
    let channels = if has_alpha { 4 } else { 3 };
    let row_len = w as usize * channels;
    let mut buf = Vec::with_capacity(row_len * h as usize);
    for y in 0..h as usize {
        let start = y * plane.stride;
        buf.extend_from_slice(&plane.data[start..start + row_len]);
    }
    let image = if has_alpha {
        DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, buf).ok_or("tampon invalide")?)
    } else {
        DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, buf).ok_or("tampon invalide")?)
    };

    // EXIF : les 4 premiers octets donnent le décalage jusqu'à l'en-tête TIFF.
    let mut ids: Vec<ItemId> = vec![0; 1];
    let exif = if handle.metadata_block_ids(&mut ids, b"Exif") > 0 {
        handle.metadata(ids[0]).ok().and_then(|raw| {
            if raw.len() < 4 {
                return None;
            }
            let off = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
            raw.get(4 + off..).map(|t| t.to_vec())
        })
    } else {
        None
    };
    let icc = handle.color_profile_raw().map(|p| p.data);

    Ok(Loaded {
        image,
        exif: exif.map(reset_orientation),
        icc,
    })
}

fn load_generic(path: &Path) -> Result<Loaded, String> {
    let reader = ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().ok();
    let exif = decoder.exif_metadata().ok().flatten();
    let icc = decoder.icc_profile().ok().flatten();
    let mut image = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    if let Some(o) = orientation {
        image.apply_orientation(o);
    }
    Ok(Loaded {
        image,
        exif: exif.map(reset_orientation),
        icc,
    })
}

/// L'image est déjà redressée : on remet le tag EXIF Orientation (0x0112) à 1
/// pour qu'aucun logiciel ne la fasse pivoter une seconde fois.
pub fn reset_orientation(mut exif: Vec<u8>) -> Vec<u8> {
    // Certains blocs commencent par "Exif\0\0"
    let base = if exif.starts_with(b"Exif\0\0") { 6 } else { 0 };
    let t = &exif[base..];
    if t.len() < 8 {
        return exif;
    }
    let le = match &t[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return exif,
    };
    let rd16 = |b: &[u8], i: usize| -> Option<u16> {
        let s = b.get(i..i + 2)?;
        Some(if le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) })
    };
    let rd32 = |b: &[u8], i: usize| -> Option<u32> {
        let s = b.get(i..i + 4)?;
        let a = [s[0], s[1], s[2], s[3]];
        Some(if le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    };
    let Some(ifd) = rd32(t, 4).map(|v| v as usize) else { return exif };
    let Some(count) = rd16(t, ifd) else { return exif };
    for n in 0..count as usize {
        let entry = ifd + 2 + n * 12;
        if rd16(t, entry) == Some(0x0112) && rd16(t, entry + 2) == Some(3) {
            let pos = base + entry + 8;
            let one: [u8; 2] = if le { 1u16.to_le_bytes() } else { 1u16.to_be_bytes() };
            if pos + 2 <= exif.len() {
                exif[pos..pos + 2].copy_from_slice(&one);
            }
            break;
        }
    }
    exif
}

// ---------------------------------------------------------------------------
// Écriture
// ---------------------------------------------------------------------------

/// Aplatit la transparence sur fond blanc (pour JPEG / PDF / BMP).
fn flatten_rgb(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (o, p) in out.pixels_mut().zip(rgba.pixels()) {
        let a = p[3] as u32;
        for c in 0..3 {
            o[c] = ((p[c] as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        }
    }
    out
}

fn encode_jpeg(img: &DynamicImage, exif: Option<&[u8]>, icc: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let rgb = flatten_rgb(img);
    let mut bytes = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY);
    if let Some(e) = exif {
        let _ = enc.set_exif_metadata(e.to_vec());
    }
    if let Some(p) = icc {
        let _ = enc.set_icc_profile(p.to_vec());
    }
    enc.write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Normalise en 8 bits RGB ou RGBA (les encodeurs n'acceptent pas tous les modes).
fn to_rgb_or_rgba(img: &DynamicImage) -> DynamicImage {
    if img.color().has_alpha() {
        DynamicImage::ImageRgba8(img.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(img.to_rgb8())
    }
}

pub fn save(loaded: &Loaded, fmt: OutputFormat, dest: &Path) -> Result<(), String> {
    let exif = loaded.exif.as_deref();
    let icc = loaded.icc.as_deref();
    let img = &loaded.image;

    if fmt == OutputFormat::Pdf {
        let page = PdfPage::from_image(img)?;
        return write_pdf(dest, &[page]);
    }
    if fmt == OutputFormat::Jpeg {
        let bytes = encode_jpeg(img, exif, icc)?;
        return fs::write(dest, bytes).map_err(|e| e.to_string());
    }

    let file = File::create(dest).map_err(|e| e.to_string())?;
    let mut w = BufWriter::new(file);
    let res = match fmt {
        OutputFormat::Png => {
            let im = to_rgb_or_rgba(img);
            let mut enc = PngEncoder::new(&mut w);
            if let Some(p) = icc {
                let _ = enc.set_icc_profile(p.to_vec());
            }
            if let Some(e) = exif {
                let _ = enc.set_exif_metadata(e.to_vec());
            }
            enc.write_image(im.as_bytes(), im.width(), im.height(), im.color().into())
        }
        OutputFormat::Webp => {
            let im = to_rgb_or_rgba(img);
            let mut enc = WebPEncoder::new_lossless(&mut w);
            if let Some(p) = icc {
                let _ = enc.set_icc_profile(p.to_vec());
            }
            if let Some(e) = exif {
                let _ = enc.set_exif_metadata(e.to_vec());
            }
            enc.write_image(im.as_bytes(), im.width(), im.height(), im.color().into())
        }
        OutputFormat::Tiff => {
            let im = to_rgb_or_rgba(img);
            let mut buf = Cursor::new(Vec::new());
            let r = TiffEncoder::new(&mut buf).write_image(
                im.as_bytes(),
                im.width(),
                im.height(),
                im.color().into(),
            );
            r.and_then(|_| w.write_all(buf.get_ref()).map_err(image::ImageError::IoError))
        }
        OutputFormat::Bmp => {
            let im = DynamicImage::ImageRgb8(flatten_rgb(img));
            let mut buf = Cursor::new(Vec::new());
            im.write_to(&mut buf, ImageFormat::Bmp)
                .and_then(|_| w.write_all(buf.get_ref()).map_err(image::ImageError::IoError))
        }
        OutputFormat::Gif => {
            let im = DynamicImage::ImageRgba8(img.to_rgba8());
            let mut buf = Cursor::new(Vec::new());
            im.write_to(&mut buf, ImageFormat::Gif)
                .and_then(|_| w.write_all(buf.get_ref()).map_err(image::ImageError::IoError))
        }
        OutputFormat::Jpeg | OutputFormat::Pdf => unreachable!(),
    };
    res.map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// PDF minimal (une image JPEG par page A4, centrée)
// ---------------------------------------------------------------------------

pub struct PdfPage {
    jpeg: Vec<u8>,
    width: u32,
    height: u32,
}

impl PdfPage {
    pub fn from_image(img: &DynamicImage) -> Result<Self, String> {
        Ok(Self {
            jpeg: encode_jpeg(img, None, None)?,
            width: img.width(),
            height: img.height(),
        })
    }
}

pub fn write_pdf(dest: &Path, pages: &[PdfPage]) -> Result<(), String> {
    const A4: (f64, f64) = (595.28, 841.89);
    const MARGIN: f64 = 28.35; // 1 cm

    let mut out: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    // Objets : 1 = Catalog, 2 = Pages, puis 3 objets par page (Page, Contents, Image)
    let page_ids: Vec<usize> = (0..pages.len()).map(|i| 3 + i * 3).collect();

    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        let id = offsets.len();
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: String = page_ids.iter().map(|id| format!("{id} 0 R ")).collect();
    obj(
        &mut out,
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).as_bytes(),
    );

    for (i, p) in pages.iter().enumerate() {
        let id = page_ids[i];
        let (pw, ph) = if p.width > p.height { (A4.1, A4.0) } else { A4 };
        let scale = ((pw - 2.0 * MARGIN) / p.width as f64).min((ph - 2.0 * MARGIN) / p.height as f64);
        let (dw, dh) = (p.width as f64 * scale, p.height as f64 * scale);
        let (x, y) = ((pw - dw) / 2.0, (ph - dh) / 2.0);

        obj(
            &mut out,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] \
                 /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>",
                id + 2,
                id + 1
            )
            .as_bytes(),
        );
        let content = format!("q {dw:.3} 0 0 {dh:.3} {x:.3} {y:.3} cm /Im0 Do Q");
        obj(
            &mut out,
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).as_bytes(),
        );
        let mut img_obj = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            p.width,
            p.height,
            p.jpeg.len()
        )
        .into_bytes();
        img_obj.extend_from_slice(&p.jpeg);
        img_obj.extend_from_slice(b"\nendstream");
        obj(&mut out, &img_obj);
    }

    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    fs::write(dest, out).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Traitement par lot
// ---------------------------------------------------------------------------

/// Choisit un nom de fichier libre : photo.jpg -> photo (1).jpg, en tenant compte
/// des noms déjà réservés pendant ce lot.
fn unique_path(dir: &Path, stem: &str, ext: &str, reserved: &mut HashSet<PathBuf>) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    let mut i = 1;
    while candidate.exists() || reserved.contains(&candidate) {
        candidate = dir.join(format!("{stem} ({i}).{ext}"));
        i += 1;
    }
    reserved.insert(candidate.clone());
    candidate
}

pub struct Progress {
    pub done: AtomicUsize,
    pub cancel: AtomicBool,
    pub current: Mutex<String>,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            done: AtomicUsize::new(0),
            cancel: AtomicBool::new(false),
            current: Mutex::new(String::new()),
        }
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Convertit toutes les images (en parallèle). Les images non traitées à cause
/// d'une annulation ne figurent pas dans le résultat.
pub fn convert_batch(
    sources: &[PathBuf],
    out_dir: &Path,
    fmt: OutputFormat,
    merge_pdf: bool,
    progress: &Progress,
) -> Vec<ConvResult> {
    if let Err(e) = fs::create_dir_all(out_dir) {
        return sources
            .iter()
            .map(|s| ConvResult {
                source: s.clone(),
                output: None,
                error: Some(format!("dossier de destination inaccessible : {e}")),
            })
            .collect();
    }

    let step = |src: &Path| {
        *progress.current.lock().unwrap() = name_of(src);
    };
    let finish = || {
        progress.done.fetch_add(1, Ordering::Relaxed);
    };

    if fmt == OutputFormat::Pdf && merge_pdf {
        let pages: Vec<(PathBuf, Option<Result<PdfPage, String>>)> = sources
            .par_iter()
            .map(|src| {
                if progress.cancel.load(Ordering::Relaxed) {
                    return (src.clone(), None);
                }
                step(src);
                let r = load(src).and_then(|l| PdfPage::from_image(&l.image));
                finish();
                (src.clone(), Some(r))
            })
            .collect();

        let mut results = Vec::new();
        let mut good = Vec::new();
        for (src, r) in pages {
            match r {
                None => {}
                Some(Ok(p)) => {
                    good.push(p);
                    results.push(ConvResult { source: src, output: None, error: None });
                }
                Some(Err(e)) => results.push(ConvResult { source: src, output: None, error: Some(e) }),
            }
        }
        if !good.is_empty() {
            let dest = unique_path(out_dir, "images_converties", "pdf", &mut HashSet::new());
            match write_pdf(&dest, &good) {
                Ok(()) => {
                    for r in results.iter_mut().filter(|r| r.ok()) {
                        r.output = Some(dest.clone());
                    }
                }
                Err(e) => {
                    for r in results.iter_mut().filter(|r| r.ok()) {
                        r.error = Some(format!("écriture du PDF impossible : {e}"));
                    }
                }
            }
        }
        return results;
    }

    // Réserve les noms de sortie à l'avance pour éviter les collisions entre threads.
    let mut reserved = HashSet::new();
    let jobs: Vec<(PathBuf, PathBuf)> = sources
        .iter()
        .map(|src| {
            let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
            let dest = unique_path(out_dir, &stem, fmt.extension(), &mut reserved);
            (src.clone(), dest)
        })
        .collect();

    jobs.par_iter()
        .filter_map(|(src, dest)| {
            if progress.cancel.load(Ordering::Relaxed) {
                return None;
            }
            step(src);
            let r = load(src).and_then(|l| save(&l, fmt, dest));
            if r.is_err() {
                let _ = fs::remove_file(dest);
            }
            finish();
            Some(match r {
                Ok(()) => ConvResult { source: src.clone(), output: Some(dest.clone()), error: None },
                Err(e) => ConvResult { source: src.clone(), output: None, error: Some(e) },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("heicdrop-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn collects_only_supported_files() {
        let files = collect_images([fixtures()]);
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn heic_to_every_format() {
        let src = vec![fixtures().join("photo1.heic"), fixtures().join("alpha.heic"), fixtures().join("plain.jpg")];
        for fmt in OutputFormat::ALL {
            let out = tmp(fmt.label());
            let res = convert_batch(&src, &out, fmt, false, &Progress::default());
            assert_eq!(res.len(), 3);
            for r in &res {
                assert!(r.ok(), "{fmt:?} {:?}: {:?}", r.source, r.error);
                let p = r.output.as_ref().unwrap();
                assert!(fs::metadata(p).unwrap().len() > 0);
                if fmt != OutputFormat::Pdf {
                    image::open(p).expect("le fichier produit doit être lisible");
                }
            }
        }
    }

    #[test]
    fn heic_dimensions_and_alpha() {
        let l = load(&fixtures().join("alpha.heic")).unwrap();
        assert!(l.image.color().has_alpha());
        assert_eq!((l.image.width(), l.image.height()), (120, 80));
    }

    #[test]
    fn merged_pdf_and_errors() {
        let src = vec![fixtures().join("photo1.heic"), fixtures().join("broken.heic"), fixtures().join("plain.jpg")];
        let out = tmp("merge");
        let res = convert_batch(&src, &out, OutputFormat::Pdf, true, &Progress::default());
        assert_eq!(res.iter().filter(|r| r.ok()).count(), 2);
        assert!(!res[1].ok());
        let pdf = fs::read(out.join("images_converties.pdf")).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert_eq!(pdf.windows(10).filter(|w| w == b"/Type /Pag").count(), 3); // Pages + 2 Page
    }

    #[test]
    fn name_collisions_get_suffix() {
        let src = vec![fixtures().join("photo1.heic"), fixtures().join("photo1.heic")];
        let out = tmp("dup");
        let res = convert_batch(&src, &out, OutputFormat::Png, false, &Progress::default());
        assert!(out.join("photo1.png").exists() && out.join("photo1 (1).png").exists(), "{res:?}");
    }

    #[test]
    fn orientation_tag_is_reset() {
        // TIFF little-endian, 1 entrée : Orientation = 6
        let mut t = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
        t = reset_orientation(t);
        assert_eq!(t[18], 1);
    }
}
