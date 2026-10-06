use std::io::Cursor;
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use rusqlite::{Connection, MAIN_DB};

use super::qt_stream::QtReader;
use super::read_v2::embedded_database;
use super::write_v2::{APPLICATION_ID, USER_VERSION};
use super::{
    PureRefBoard, PureRefImage, PureRefImageItem, PureRefNote, PureRefPixelRect, PureRefTransform,
    read_board, write_board,
};

#[test]
fn written_board_reads_back_unchanged() {
    let board = sample_board();

    let read = read_board(&write_board(&board).unwrap()).unwrap();

    assert_eq!(read.images, board.images);
    assert_boards_match(&board, &read);
}

#[test]
fn written_file_has_pureref_container_layout() {
    let file = write_board(&sample_board()).unwrap();

    let mut header = QtReader::new(&file);
    let format = header.string().unwrap().unwrap();
    assert_eq!(format, "2.1");
    assert_eq!(header.u32().unwrap(), 0);
    let database_len = header.u64().unwrap() as usize;
    assert_eq!(header.string().unwrap().as_deref(), Some("2.0.3"));
    let checksum = header.string().unwrap().unwrap();
    assert_eq!(
        checksum,
        hex::encode(Md5::digest(&file[header.position()..]))
    );
    assert_eq!(header.byte_array().unwrap(), Some(&[][..]));
    assert_eq!(file.len(), database_len + header.position());

    let database = embedded_database(&file, &format).unwrap();
    assert!(database.starts_with(b"SQLite format 3\0"));
    assert_eq!(database[18..20], [1, 1], "rollback journal, not WAL");
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .deserialize_read_exact(MAIN_DB, database.as_slice(), database.len(), true)
        .unwrap();
    let pragma = |name: &str| -> i64 {
        connection
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(pragma("application_id"), APPLICATION_ID);
    assert_eq!(pragma("user_version"), USER_VERSION);
    assert_eq!(pragma("page_size"), 4096);
    assert_eq!(pragma("auto_vacuum"), 1);

    for (table, column) in [
        ("items", "transform"),
        ("items", "sort_order"),
        ("items_images", "image_transform"),
        ("items_images", "image_bounds"),
        ("items_notes", "fixed_size"),
        ("metadata", "view_transform"),
    ] {
        let classes: Vec<String> = connection
            .prepare(&format!("SELECT DISTINCT typeof({column}) FROM {table}"))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(classes, ["text"], "{table}.{column}");
    }
}

/// Reads the unlicensed PureRef-authored samples, which stay outside the repo.
#[test]
#[ignore = "needs PureRef samples in /tmp/pur-samples/real"]
fn reads_real_pureref_samples() {
    let root = Path::new("/tmp/pur-samples/real");
    let mut files = Vec::new();
    collect_pur_files(root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no samples under {}", root.display());

    let mut failures = Vec::new();
    for path in &files {
        let name = path.strip_prefix(root).unwrap().display();
        let board = match read_board(&std::fs::read(path).unwrap()) {
            Ok(board) => board,
            Err(error) => {
                eprintln!("{name}: ERROR {error}");
                failures.push(name.to_string());
                continue;
            }
        };
        let linked = board.images.iter().filter(|image| image.bytes.is_none());
        eprintln!(
            "{name}: {} images ({} linked), {} image items, {} notes",
            board.images.len(),
            linked.count(),
            board.image_items.len(),
            board.notes.len(),
        );
        for item in &board.image_items {
            let image = &board.images[item.image];
            assert!(item.crop.width <= f64::from(image.width), "{name}");
            assert!(item.crop.height <= f64::from(image.height), "{name}");
        }
        let rewritten = read_board(&write_board(&board).unwrap()).unwrap();
        assert_boards_match(&board, &rewritten);
    }
    assert!(failures.is_empty(), "failed: {failures:?}");

    let image = read_sample("pureref2-fyordev/01-image.pur");
    let item = &image.image_items[0];
    assert_eq!((image.images[0].width, image.images[0].height), (64, 32));
    assert_point(item.transform.map_point(32.0, 16.0), (100.0, 200.0));
    assert_rect(item.crop, full_rect(64, 32));

    let rotated = read_sample("pureref1-mnmly/test-scene-rot-90-2item.pur");
    assert_eq!(rotated.image_items.len(), 5);
    assert_eq!(rotated.images.len(), 2);
    // The scaled duplicate stores image_transform translate(-1.5, -1.5) on a 1x1 image.
    let scaled_duplicate = &rotated.image_items[4];
    assert_eq!(scaled_duplicate.image, rotated.image_items[2].image);
    assert_point(
        scaled_duplicate.transform.map_point(0.5, 0.5),
        (5.628425693724097, -1.1802911037102346),
    );
    assert_rect(scaled_duplicate.crop, full_rect(1, 1));
}

fn sample_board() -> PureRefBoard {
    let angle = 30_f64.to_radians();
    let scale = 2.5;
    let rotated = PureRefTransform {
        m11: scale * angle.cos(),
        m12: scale * angle.sin(),
        m21: -scale * angle.sin(),
        m22: scale * angle.cos(),
        dx: -40.0,
        dy: 12.5,
    };
    PureRefBoard {
        images: vec![
            image(png(6, 4), Some("C:/refs/red.png")),
            image(png(3, 5), None),
            PureRefImage {
                bytes: None,
                source: Some("/refs/missing.jpg".to_owned()),
                width: 10,
                height: 8,
            },
        ],
        image_items: vec![
            image_item(
                0,
                Some("red"),
                PureRefTransform::translation(10.0, 20.0),
                full_rect(6, 4),
            ),
            PureRefImageItem {
                opacity: 0.5,
                ..image_item(
                    0,
                    None,
                    rotated,
                    PureRefPixelRect {
                        x: 1.0,
                        y: 0.5,
                        width: 3.0,
                        height: 2.5,
                    },
                )
            },
            image_item(1, Some("blue"), PureRefTransform::IDENTITY, full_rect(3, 5)),
            image_item(
                2,
                Some("missing"),
                PureRefTransform::translation(-5.0, 0.0),
                full_rect(10, 8),
            ),
        ],
        notes: vec![PureRefNote {
            text: "Tom & \"Jerry\" <3\n\n  indented".to_owned(),
            transform: PureRefTransform {
                m11: 2.0,
                m22: 2.0,
                ..PureRefTransform::translation(7.0, -311.0)
            },
        }],
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([200, 40, 40, 255]))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn image(bytes: Vec<u8>, source: Option<&str>) -> PureRefImage {
    let (width, height) = image::load_from_memory(&bytes)
        .unwrap()
        .into_rgba8()
        .dimensions();
    PureRefImage {
        bytes: Some(bytes),
        source: source.map(str::to_owned),
        width,
        height,
    }
}

fn image_item(
    image: usize,
    name: Option<&str>,
    transform: PureRefTransform,
    crop: PureRefPixelRect,
) -> PureRefImageItem {
    PureRefImageItem {
        image,
        name: name.map(str::to_owned),
        transform,
        crop,
        opacity: 1.0,
    }
}

fn full_rect(width: u32, height: u32) -> PureRefPixelRect {
    PureRefPixelRect {
        x: 0.0,
        y: 0.0,
        width: f64::from(width),
        height: f64::from(height),
    }
}

fn assert_boards_match(expected: &PureRefBoard, actual: &PureRefBoard) {
    assert_eq!(actual.images.len(), expected.images.len());
    assert_eq!(actual.image_items.len(), expected.image_items.len());
    for (expected, actual) in expected.image_items.iter().zip(&actual.image_items) {
        assert_eq!(
            (actual.image, &actual.name, actual.opacity),
            (expected.image, &expected.name, expected.opacity)
        );
        assert_transform(actual.transform, expected.transform);
        assert_rect(actual.crop, expected.crop);
    }
    assert_eq!(actual.notes.len(), expected.notes.len());
    for (expected, actual) in expected.notes.iter().zip(&actual.notes) {
        assert_eq!(actual.text, expected.text);
        assert_transform(actual.transform, expected.transform);
    }
}

fn assert_transform(actual: PureRefTransform, expected: PureRefTransform) {
    let pairs = [
        (actual.m11, expected.m11),
        (actual.m12, expected.m12),
        (actual.m21, expected.m21),
        (actual.m22, expected.m22),
        (actual.dx, expected.dx),
        (actual.dy, expected.dy),
    ];
    assert!(
        pairs.iter().all(|(a, e)| (a - e).abs() < 1e-9),
        "{actual:?} != {expected:?}"
    );
}

fn assert_rect(actual: PureRefPixelRect, expected: PureRefPixelRect) {
    let pairs = [
        (actual.x, expected.x),
        (actual.y, expected.y),
        (actual.width, expected.width),
        (actual.height, expected.height),
    ];
    assert!(
        pairs.iter().all(|(a, e)| (a - e).abs() < 1e-9),
        "{actual:?} != {expected:?}"
    );
}

fn assert_point(actual: (f64, f64), expected: (f64, f64)) {
    assert!(
        (actual.0 - expected.0).abs() < 1e-9 && (actual.1 - expected.1).abs() < 1e-9,
        "{actual:?} != {expected:?}"
    );
}

fn read_sample(relative: &str) -> PureRefBoard {
    read_board(&std::fs::read(Path::new("/tmp/pur-samples/real").join(relative)).unwrap()).unwrap()
}

fn collect_pur_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_pur_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "pur") {
            files.push(path);
        }
    }
}
