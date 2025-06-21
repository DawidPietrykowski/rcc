// use crate::xmp::read_rating_xmp;
use anyhow::{Error, Result, anyhow, bail};
use chrono::DateTime;
use clap::{Parser, Subcommand, ValueEnum};
use rexiv2::Metadata;
use std::ffi::{OsStr, OsString};
use std::fmt::Debug;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::BufReader;
use std::ops::{Mul, Sub};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fmt, fs, io};

mod xmp;

const IMAGE_EXTENSIONS: [&str; 3] = ["heic", "jpg", "jpeg"];
const VIDEOS_EXTENSIONS: [&str; 3] = ["mov", "mp4", "avi"];

#[derive(Clone, Eq, PartialEq, Debug)]
struct CollectedMetadata {
    base_file_name: String,
    file_size: u64,
    extension: String,
    creation_date: Option<String>,
    video_duration: Option<Duration>,
}

#[derive(Parser)]
struct Cli {
    // #[command(subcommand)]
    // command: FileCommand,
    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,

    #[arg(short = 'e', long)]
    exclude: Vec<String>,

    #[arg(short = 'f', long, default_value_t = false)]
    flip_exclusion: bool,

    #[arg(short = 'a', long, default_value_t = true)]
    include_videos: bool,

    #[arg(short, long)]
    dest: PathBuf,

    #[arg(short, long, value_delimiter = ' ', num_args = 1..)]
    src: Vec<PathBuf>,
}

#[derive(Subcommand, PartialEq)]
enum FileCommand {
    Move,
    Copy,
    Delete,
    Print,
}

impl Display for ComparisonCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ComparisonCommand::MoreEqual => write!(f, "more-equal"),
            ComparisonCommand::LessEqual => write!(f, "less-equal"),
            ComparisonCommand::Equal => write!(f, "equal"),
        }
    }
}

#[derive(ValueEnum, Clone, Debug)]
enum ComparisonCommand {
    MoreEqual,
    LessEqual,
    Equal,
}

#[derive(Clone, Eq, PartialEq, Debug)]
struct Entry {
    path: PathBuf,
    metadata: CollectedMetadata,
    is_dest: bool,
}

fn entries_match(a: &CollectedMetadata, b: &CollectedMetadata) -> bool {
    if let Some(date) = compare_if_exist(&a.creation_date, &b.creation_date) {
        if !date {
            return false;
        }
        // println!("match on video date");
    } else {
        // dates required
        return false;
    }

    if let Some(date) = compare_if_exist(&a.video_duration, &b.video_duration) {
        if !date {
            // println!("mismatch on video duration");
            return false;
        }
        // println!("match on video duration");
    }

    if !compare_with_tolerance(a.file_size as f32, b.file_size as f32) {
        // println!("mismatch on size");
        return false;
    }

    if a.extension != b.extension {
        return false;
    }

    return true;
}

fn compare_if_exist<T: PartialEq>(a: &Option<T>, b: &Option<T>) -> Option<bool> {
    if let (Some(a_date), Some(b_date)) = (a, b) {
        return Some(a_date == b_date);
    }
    None
}

const TOLERANCE: f32 = 0.01;
fn compare_with_tolerance<T: PartialEq + Sub + Mul<f32> + Copy + PartialOrd>(a: T, b: T) -> bool
where
    <T as Sub>::Output: PartialOrd<<T as Mul<f32>>::Output>,
    <T as Mul<f32>>::Output: Debug,
    <T as Sub>::Output: Debug,
{
    let max = if a > b { a } else { b };
    let min = if a > b { b } else { a };
    let max_diff = max * TOLERANCE;
    let diff = max - min;
    // println!("diff: {:?} max: {:?}", diff, max_diff);
    return diff < max_diff;
}

// fn handle_duplicate(entry: &Entry) {
// }

fn main() {
    let cli: Cli = Cli::parse();

    rexiv2::initialize().expect("Unable to initialize rexiv2");

    let src_entries = scan_directories(&cli.src, false, &cli);
    let dest_entries = scan_directories(&vec![cli.dest.clone()], true, &cli);

    println!("\nSearching for duplicates\n");

    let mut saved_space = 0u64;

    for dest_entry in dest_entries {
        for src_entry in &src_entries {
            if *src_entry.path == dest_entry.path {
                println!(
                    "File is both in source and destination directories: {:?}",
                    dest_entry.path
                );
                continue;
            }
            if entries_match(&dest_entry.metadata, &src_entry.metadata) {
                println!(
                    "Duplicate found for: {}: {}",
                    dest_entry.path.display(),
                    src_entry.path.display()
                );
                saved_space += src_entry.metadata.file_size;
                // handle_duplicate(src_entry);
            } else if dest_entry.metadata.base_file_name == src_entry.metadata.base_file_name {
                println!(
                    "Files have the same base name but did not match: \n\n{:?}\n\n{:?}",
                    dest_entry, src_entry
                );
            }
        }
    }

    let saved_mb = saved_space / (1024 * 1024);
    let size_str = if saved_mb >= 1024 {
        format!("{}.{}GB", saved_mb / 1024, saved_mb % 1024)
    } else {
        format!("{}MB", saved_mb)
    };
    println!("Total saved space: {}", size_str);
}

fn scan_directories(dir_paths: &Vec<PathBuf>, is_dest: bool, cli: &Cli) -> Vec<Entry> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for path in dir_paths {
        visit_dirs(
            path.to_path_buf(),
            &mut paths,
            0,
            cli.exclude.clone(),
            cli.flip_exclusion,
            cli.include_videos,
            false,
        )
        .expect("Failed to iterate over directories");
    }
    let mut entries = Vec::new();
    for path in paths {
        let res: Result<CollectedMetadata> = get_metadata(&path);
        let Ok(metadata) = res else {
            println!(
                "Skipping {path:?} due to {}",
                res.err().unwrap_or(anyhow!("Unknown error")).to_string()
            );
            continue;
        };

        let entry = Entry {
            path,
            metadata,
            is_dest,
        };

        println!("Adding: {:?}", entry);

        entries.push(entry)

        // let mut should_move = pass_treshold_check && pass_label_check;

        // if cli.inverse {
        //     should_move = !should_move;
        // }

        // if should_move {
        // let path_str = path.as_os_str().to_str().unwrap();

        // if cli.verbose {
        //     println!("Rated: {rating} {command_name} {path:?}");
        // }

        // let mut new_file_path: Option<PathBuf> = None;
        // if cli.command == FileCommand::Move || cli.command == FileCommand::Copy {
        //     new_file_path = Some(output_path.clone().unwrap().join(&relative_path));
        //     let new_file_path_clone = new_file_path.clone().unwrap();
        //     let dir_path: &Path = new_file_path_clone.parent().unwrap();
        //     if !path_exists(dir_path.to_path_buf()) {
        //         fs::create_dir(dir_path.to_path_buf()).unwrap();
        //     }
        // }

        // apply_command(
        //     &cli.command,
        //     cli.verbose,
        //     path.clone(),
        //     new_file_path.clone(),
        // );
        // if cli.match_raws && (path_str.contains(".jpg") || path_str.contains(".JPG")) {
        //     let mut raw_path = path.clone();
        //     raw_path.set_extension("ARW");

        //     if raw_path.exists() {
        //         if cli.verbose {
        //             println!("Matched raw file {raw_path:?}");
        //         }
        //         let raw_relative_path = raw_path
        //             .strip_prefix(search_path.clone())
        //             .expect(format!("Failed to strip root prefix of file {:?}", path).as_str());
        //         let new_raw_file_path: Option<PathBuf> = if output_path.is_none() {
        //             None
        //         } else {
        //             Some(output_path.clone().unwrap().join(&raw_relative_path))
        //         };
        //         apply_command(&cli.command, cli.verbose, raw_path, new_raw_file_path);
        //     }
        // }
        // }
    }
    entries
}

fn visit_dirs(
    dir: PathBuf,
    paths: &mut Vec<PathBuf>,
    depth: i32,
    excluded_paths: Vec<String>,
    flip_exclusion: bool,
    include_videos: bool,
    print_directories: bool,
) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path
                    .as_path()
                    .file_name()
                    .expect("Could not get relative path")
                    .to_str()
                    .unwrap();
                let mut filter_res = filter_string(dir_name, excluded_paths.clone());
                if flip_exclusion {
                    filter_res = !filter_res;
                }
                if (depth != 0 || filter_res) && !dir_name.starts_with(".") {
                    // filter
                    if print_directories && depth == 0 {
                        println!("Including {dir_name}");
                    }
                    visit_dirs(
                        path,
                        paths,
                        depth + 1,
                        excluded_paths.clone(),
                        flip_exclusion,
                        include_videos,
                        print_directories,
                    )?;
                }
            } else {
                let path_buf = entry.path();
                if is_file_allowed(&path_buf, include_videos) {
                    // println!("Adding {path_buf:?}");
                    paths.push(path_buf);
                } else {
                    // println!("Skipping {path_buf:?}");
                }
            }
        }
    } else if dir.is_file() {
        paths.push(dir);
    }

    Ok(())
}

fn filter_string(string: &str, excluded_paths: Vec<String>) -> bool {
    for path in excluded_paths {
        if string.contains(&path) {
            return false;
        }
    }
    true
}

fn path_exists(path: PathBuf) -> bool {
    fs::metadata(path).is_ok()
}

fn is_file_allowed(filename: &PathBuf, include_videos: bool) -> bool {
    if filename
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(".")
    {
        return false;
    }

    let ext = filename
        .extension()
        .unwrap_or(OsStr::new(""))
        .to_str()
        .unwrap();
    let lower_passed = ext.to_lowercase();

    let mut ext: Vec<&str> = IMAGE_EXTENSIONS.to_vec();

    if include_videos {
        ext.extend(VIDEOS_EXTENSIONS.iter());
    }

    for allowed_extension in ext {
        let lower_allowed = allowed_extension.to_lowercase();
        if lower_allowed == lower_passed {
            return true;
        }
    }
    false
}

fn get_metadata(filename: &PathBuf) -> Result<CollectedMetadata> {
    Ok(CollectedMetadata {
        base_file_name: filename
            .file_name()
            .ok_or(Error::msg("File metadata read error"))?
            .to_os_string()
            .into_string()
            .unwrap(),
        file_size: filename.metadata()?.size(),
        creation_date: get_timestamp(filename).ok(),
        video_duration: get_mp4_duration(filename).ok(),
        extension: filename
            .extension()
            .unwrap()
            .to_os_string()
            .into_string()
            .unwrap(),
    })
}
fn get_timestamp(filename: &PathBuf) -> Result<String> {
    if !path_exists(filename.clone()) {
        anyhow::bail!("File doesn't exist");
    }

    if is_video(&filename) {
        // return read_timestamp_xmp(filename.clone());
        return get_mp4_timestamp(filename);
    }

    // Use rexiv2 for image files
    let meta = Metadata::new_from_path(filename);
    match meta {
        Ok(meta) => {
            // println!("TAGS: {:?}", meta.get_exif_tags());
            if let Ok(rating) = meta.get_tag_string("Exif.Photo.DateTimeOriginal") {
                return Ok(rating);
            } else {
                anyhow::bail!("Not found");
            }
        }
        Err(e) => anyhow::bail!(e),
    }
}

fn is_video(path: &Path) -> bool {
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_str()
        .unwrap_or("")
        .to_lowercase();
    VIDEOS_EXTENSIONS.contains(&extension.as_str())
}

fn get_mp4_timestamp(filename: &PathBuf) -> Result<String> {
    let mp4 = read_mp4_video(filename)?;

    if mp4.moov.mvhd.creation_time == 0 {
        bail!("no creation time");
    }
    let dt = DateTime::from_timestamp(
        (mp4.moov.mvhd.creation_time - MP4_TO_UNIX_OFFSET)
            .try_into()
            .unwrap(),
        0,
    )
    .expect("invalid timestamp");
    Ok(dt.to_string())
}

fn get_mp4_size(filename: &PathBuf) -> Result<u64> {
    let mp4 = read_mp4_video(filename)?;

    Ok(mp4.size())
}

fn get_mp4_duration(filename: &PathBuf) -> Result<Duration> {
    let mp4 = read_mp4_video(filename)?;

    Ok(mp4.duration())
}

const MP4_TO_UNIX_OFFSET: u64 = 2_082_844_800;
fn read_mp4_video(filename: &PathBuf) -> Result<mp4::Mp4Reader<BufReader<File>>, anyhow::Error> {
    let f = File::open(filename).unwrap();
    let size = f.metadata()?.len();
    let reader = BufReader::new(f);
    let mp4 = mp4::Mp4Reader::read_header(reader, size)?;
    Ok(mp4)
}
