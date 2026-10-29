const MAX_EDGE: u32 = 256;

pub fn jpeg_thumbnail(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|_| "file is not a supported image (jpeg, png, or gif)".to_string())?;
    let thumb = image.thumbnail(MAX_EDGE, MAX_EDGE).to_rgb8();
    encode_jpeg(&thumb)
}

pub fn solid_jpeg(color: [u8; 3]) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(64, 64, image::Rgb(color));
    encode_jpeg(&image).expect("solid color jpeg")
}

pub fn seed_color(name: &str) -> [u8; 3] {
    match name {
        "Orc Warrior" => [148, 72, 42],
        "Orc Archer" => [96, 118, 52],
        "Goblin Spearman" => [62, 122, 74],
        "Human Wizard" => [48, 72, 138],
        "Skeleton Warrior" => [130, 128, 120],
        "Elf Scout" => [42, 122, 118],
        _ => [90, 86, 78],
    }
}

fn encode_jpeg(image: &image::RgbImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
    encoder
        .encode(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|_| "could not encode thumbnail".to_string())?;
    Ok(out)
}

/// Accept a Drive file id or a drive.google.com file link.
/// Photos links and any other host are rejected so the server never fetches an arbitrary URL.
pub fn parse_drive_file_id(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("a Drive file id or link is required".into());
    }
    let lower = input.to_ascii_lowercase();
    if lower.contains("photos.google.") || lower.contains("photos.app.goo.gl") {
        return Err(
            "Google Photos links are not supported. This app does not use Google OAuth. Upload the image, or use a Google Drive file shared as anyone with the link."
                .into(),
        );
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return id_from_drive_url(input);
    }
    validate_id(input)
}

fn id_from_drive_url(input: &str) -> Result<String, String> {
    let rest = input
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(input);
    let (host_and_path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let host = host_and_path.split('/').next().unwrap_or("");
    let host = host.split('@').next_back().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    if host != "drive.google.com" && host != "docs.google.com" {
        return Err("only a Google Drive file id or a drive.google.com link is accepted".into());
    }
    let path = host_and_path
        .split_once('/')
        .map(|(_, path)| path)
        .unwrap_or("");
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if let Some(index) = parts.iter().position(|part| *part == "d") {
        if let Some(id) = parts.get(index + 1) {
            return validate_id(id);
        }
    }
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key == "id" && !value.is_empty() {
            return validate_id(value);
        }
    }
    Err("could not find a Drive file id in that link".into())
}

fn validate_id(id: &str) -> Result<String, String> {
    let ok = (10..=128).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(id.to_string())
    } else {
        Err("that Drive file id is not valid".into())
    }
}

pub fn google_download_host_ok(host: &str) -> bool {
    host == "drive.google.com"
        || host == "docs.google.com"
        || host == "googleusercontent.com"
        || host.ends_with(".google.com")
        || host.ends_with(".googleusercontent.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscales_a_large_png_and_returns_jpeg() {
        let image = image::RgbImage::from_fn(800, 600, |x, y| {
            image::Rgb([(x % 255) as u8, (y % 200) as u8, 30])
        });
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let jpeg = jpeg_thumbnail(&png).unwrap();
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert!(decoded.width() <= 256);
        assert!(decoded.height() <= 256);
        assert!(decoded.width() == 256 || decoded.height() == 256);
        assert!(jpeg.len() < png.len());
        assert!(jpeg.starts_with(&[0xFF, 0xD8]));
    }

    #[test]
    fn ids_hosts_and_seed_colors_follow_the_thumbnail_rules() {
        assert_eq!(validate_id("1AbC-def_90").unwrap(), "1AbC-def_90");
        assert!(validate_id("short").unwrap_err().contains("not valid"));
        assert!(validate_id(&"a".repeat(129)).is_err());
        assert_eq!(
            id_from_drive_url("https://drive.google.com/file/d/1AbC-def_9012/view").unwrap(),
            "1AbC-def_9012"
        );
        assert!(id_from_drive_url("https://drive.google.com/file/d/short/view").is_err());
        assert!(google_download_host_ok("drive.google.com"));
        assert!(google_download_host_ok("docs.google.com"));
        assert!(google_download_host_ok("googleusercontent.com"));
        assert!(google_download_host_ok("lh3.googleusercontent.com"));
        assert!(google_download_host_ok("drive.google.com"));
        assert!(!google_download_host_ok("evil.example"));
        assert!(!google_download_host_ok("google.com.evil.example"));
        let color = seed_color("Orc Archer");
        assert_eq!(color, seed_color("Orc Archer"));
        assert_ne!(color, seed_color("Goblin"));
        let jpeg = solid_jpeg(color);
        assert!(jpeg.starts_with(&[0xFF, 0xD8]));
        let image = image::RgbImage::from_pixel(2, 2, image::Rgb(color));
        let encoded = encode_jpeg(&image).unwrap();
        assert!(encoded.starts_with(&[0xFF, 0xD8]));
        assert!(encoded.len() > 10);
    }

    #[test]
    fn parses_drive_ids_and_rejects_other_urls() {
        assert_eq!(
            parse_drive_file_id("1AbC-def_9012").unwrap(),
            "1AbC-def_9012"
        );
        assert_eq!(
            parse_drive_file_id("https://drive.google.com/file/d/1AbC-def_9012/view?usp=sharing")
                .unwrap(),
            "1AbC-def_9012"
        );
        assert_eq!(
            parse_drive_file_id("https://drive.google.com/open?id=1AbC-def_9012").unwrap(),
            "1AbC-def_9012"
        );
        assert_eq!(
            parse_drive_file_id("https://drive.google.com/uc?export=download&id=1AbC-def_9012")
                .unwrap(),
            "1AbC-def_9012"
        );
        assert!(parse_drive_file_id("https://photos.google.com/share/abc")
            .unwrap_err()
            .contains("Photos"));
        assert!(parse_drive_file_id("https://evil.example/file/d/1AbC-def_9012").is_err());
        assert!(parse_drive_file_id("http://169.254.169.254/latest/meta-data").is_err());
        assert!(parse_drive_file_id("short").is_err());
        assert!(parse_drive_file_id("../etc/passwd").is_err());
    }
}
