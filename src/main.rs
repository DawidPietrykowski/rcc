use anyhow::{Error, Result, anyhow, bail};
use chrono::DateTime;
use clap::{Parser, ValueEnum};
use nom_exif::*;
use num_rational::Ratio;
use rexiv2::Metadata;
use std::ffi::OsStr;
use std::fmt::{Debug, Display};
use std::fs::File;
use std::io::{BufReader, Write};
use std::ops::{Mul, Sub};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};

mod xmp;

const IMAGE_EXTENSIONS: [&str; 3] = ["heic", "jpg", "jpeg"];
const VIDEOS_EXTENSIONS: [&str; 3] = ["mov", "mp4", "avi"];

const MP4_TO_UNIX_OFFSET: u64 = 2_082_844_800;

#[derive(Clone, Eq, PartialEq, Debug)]
struct CollectedMetadata {
    file_metadata: FileMetadata,
    image_metadata: Option<ImageMetadata>,
    video_metadata: Option<VideoMetadata>,
}

#[derive(Parser, Clone)]
struct Cli {
    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,

    #[arg(short = 'e', long)]
    exclude: Vec<String>,

    #[arg(short = 'f', long, default_value_t = false)]
    flip_exclusion: bool,

    #[arg(short = 'a', long, default_value_t = true)]
    include_videos: bool,

    #[arg(value_enum, default_value_t = CompareMode::Paranoid)]
    mode: CompareMode,

    #[arg(short = 'o', long, default_value = "run.sh")]
    output: PathBuf,

    #[arg(short = 'c', long)]
    command: Option<FileCommand>,

    #[arg(short, long)]
    dest: PathBuf,

    // #[arg(short, long, value_delimiter = ' ', num_args = 1..)]
    // src: Vec<PathBuf>,
    #[arg(short, long)]
    src: PathBuf,
}

#[derive(PartialEq, Clone, Copy, ValueEnum)]
enum FileCommand {
    Move,
    Copy,
    Delete,
    Print,
}

impl Display for FileCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileCommand::Move => f.write_str("move"),
            FileCommand::Copy => f.write_str("copy"),
            FileCommand::Delete => f.write_str("delete"),
            FileCommand::Print => f.write_str("print"),
        }
    }
}

#[derive(PartialEq, Clone, Copy, ValueEnum)]
enum CompareMode {
    Loose,
    Paranoid,
}

#[derive(Clone, Eq, PartialEq, Debug)]
struct Entry {
    path: PathBuf,
    metadata: CollectedMetadata,
    is_dest: bool,
}

impl Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("dest: {}", self.is_dest))?;
        f.write_fmt(format_args!(" p: {:?}", self.path))?;
        f.write_fmt(format_args!(" m: {}", self.metadata))?;
        Ok(())
    }
}

impl Display for CollectedMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("f: {}", self.file_metadata))?;
        if let Some(meta) = self.image_metadata.clone() {
            f.write_fmt(format_args!(" i: {}", meta))?;
        }
        if let Some(meta) = self.video_metadata.clone() {
            f.write_fmt(format_args!(" v: {}", meta))?;
        }
        Ok(())
    }
}

impl Display for FileMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("base: {}", self.base_file_name))?;
        f.write_fmt(format_args!(" s: {}", self.file_size))?;
        f.write_fmt(format_args!(" e: {}", self.extension))?;
        if let Some(date) = self.creation_date.clone() {
            f.write_fmt(format_args!(" d: {}", date))?;
        }
        Ok(())
    }
}

impl Display for VideoMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("date: {}", self.date))?;
        if let Some(duration) = self.video_duration.clone() {
            f.write_fmt(format_args!(" d: {:?}", duration))?;
        }
        Ok(())
    }
}

impl Display for ImageMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("date: {}", self.date))?;
        if let Some(model) = self.model.clone() {
            f.write_fmt(format_args!(" {}", model))?;
        }
        if let Some((x, y)) = self.resolution.clone() {
            f.write_fmt(format_args!(" {}x{}", x, y))?;
        }
        if let Some(brightness) = self.brightness.clone() {
            f.write_fmt(format_args!(" b: {}", brightness))?;
        }
        Ok(())
    }
}

#[derive(Default, Clone, Eq, PartialEq, Debug)]
struct ImageMetadata {
    date: String,
    resolution: Option<(Ratio<i32>, Ratio<i32>)>,
    model: Option<String>,
    brightness: Option<String>,
}

#[derive(Default, Clone, Eq, PartialEq, Debug)]
struct FileMetadata {
    base_file_name: String,
    file_size: u64,
    extension: String,
    creation_date: Option<String>,
}

trait CompareMetadata<T> {
    fn metadata_matches(a: &T, b: &T, mode: Cli) -> bool;
}

#[derive(Default, Clone, Eq, PartialEq, Debug)]
struct VideoMetadata {
    date: String,
    video_duration: Option<Duration>,
}

impl CompareMetadata<VideoMetadata> for VideoMetadata {
    fn metadata_matches(a: &VideoMetadata, b: &VideoMetadata, cli: Cli) -> bool {
        if a.date != b.date {
            return false;
        }

        if let Some(duration) = compare_if_exist(&a.video_duration, &b.video_duration) {
            if !duration {
                return false;
            }
        } else {
            if cli.mode == CompareMode::Paranoid {
                return false;
            }
        }

        true
    }
}

impl CompareMetadata<ImageMetadata> for ImageMetadata {
    fn metadata_matches(a: &ImageMetadata, b: &ImageMetadata, _cli: Cli) -> bool {
        if a.date != b.date {
            return false;
        }

        if let Some(model) = compare_if_exist(&a.model, &b.model) {
            if !model {
                return false;
            }
        }

        if let Some(brightness) = compare_if_exist(&a.brightness, &b.brightness) {
            if !brightness {
                return false;
            }
        }
        if let Some(resolution) = compare_if_exist(&a.resolution, &b.resolution) {
            if !resolution {
                return false;
            }
        }
        true
    }
}

impl CompareMetadata<FileMetadata> for FileMetadata {
    fn metadata_matches(a: &FileMetadata, b: &FileMetadata, cli: Cli) -> bool {
        if cli.mode == CompareMode::Paranoid && a.base_file_name != b.base_file_name {
            return false;
        }

        if !compare_with_tolerance(a.file_size as f32, b.file_size as f32) {
            // println!("mismatch on size");
            return false;
        }

        if a.extension != b.extension {
            // println!("mismatch on extension");
            return false;
        }

        true
    }
}

fn entries_match(a: &CollectedMetadata, b: &CollectedMetadata, mode: Cli) -> bool {
    if !FileMetadata::metadata_matches(&a.file_metadata, &b.file_metadata, mode.clone()) {
        return false;
    }
    let mut metadata_checked = false;
    if let (Some(a), Some(b)) = (&a.image_metadata, &b.image_metadata) {
        if !ImageMetadata::metadata_matches(a, b, mode.clone()) {
            return false;
        }
        metadata_checked = true;
    }
    if let (Some(a), Some(b)) = (&a.video_metadata, &b.video_metadata) {
        if !VideoMetadata::metadata_matches(a, b, mode.clone()) {
            return false;
        }
        metadata_checked = true;
    }

    return metadata_checked;
}

fn compare_if_exist<T: PartialEq>(a: &Option<T>, b: &Option<T>) -> Option<bool> {
    match (a, b) {
        (Some(a_val), Some(b_val)) => return Some(a_val == b_val),
        (None, None) => None,
        (None, Some(_)) => Some(false),
        (Some(_), None) => Some(false),
    }
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
    return diff < max_diff;
}

struct Action {
    entry: Entry,
    dest_entry: Entry,
    action: FileCommand,
}

fn main() {
    let cli: Cli = Cli::parse();

    rexiv2::initialize().expect("Unable to initialize rexiv2");

    let src_entries = scan_directories(&vec![cli.src.clone()], false, &cli);
    let dest_entries = scan_directories(&vec![cli.dest.clone()], true, &cli);

    println!("\nSearching for duplicates\n");

    let mut saved_space = 0u64;

    let mut actions = vec![];

    for dest_entry in dest_entries {
        for src_entry in &src_entries {
            if *src_entry.path == dest_entry.path {
                println!(
                    "File is both in source and destination directories: {:?}",
                    dest_entry.path
                );
                continue;
            }
            if entries_match(&dest_entry.metadata, &src_entry.metadata, cli.clone()) {
                println!(
                    "Duplicate found for: {}: {}",
                    dest_entry.path.display(),
                    src_entry.path.display()
                );
                if dest_entry.metadata.file_metadata.base_file_name
                    != src_entry.metadata.file_metadata.base_file_name
                {
                    println!("Files have different names");
                }

                saved_space += src_entry.metadata.file_metadata.file_size;
                if let Some(command) = cli.command {
                    actions.push(Action {
                        entry: src_entry.clone(),
                        dest_entry: dest_entry.clone(),
                        action: command,
                    });
                }
            } else if cli.verbose
                && dest_entry.metadata.file_metadata.base_file_name
                    == src_entry.metadata.file_metadata.base_file_name
            {
                println!(
                    "\nFiles have the same base name but did not match: \n{:?}\n{:?}",
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

    let Some(command) = cli.command else {
        return;
    };

    let mut execution_file = File::create(cli.output.clone()).unwrap();
    execution_file
        .write("#! /bin/env sh\n\n".as_bytes())
        .unwrap();
    execution_file
        .write_fmt(format_args!(
            "# rcc -o {:?} -c {} --src {:?} --dest {:?}\n",
            cli.output, command, cli.src, cli.dest
        ))
        .unwrap();
    execution_file
        .write_fmt(format_args!("\n# Total saved space: {}\n", size_str))
        .unwrap();
    execution_file
        .write_fmt(format_args!("\n# Total actions: {}\n", actions.len()))
        .unwrap();
    for action in actions {
        execution_file
            .write_fmt(format_args!(
                "\n# destination: {:?}\n",
                action.dest_entry.path
            ))
            .unwrap();
        match action.action {
            FileCommand::Move => todo!(),
            FileCommand::Copy => todo!(),
            FileCommand::Delete => {
                execution_file
                    .write_fmt(format_args!("rm {:?}\n", action.entry.path))
                    .unwrap();
            }
            FileCommand::Print => todo!(),
        }
    }
    let mut perms = execution_file.metadata().unwrap().permissions();
    let mode = perms.mode();
    perms.set_mode(mode | 0o1 /* execute */);
    execution_file.set_permissions(perms).unwrap();
    execution_file.flush().unwrap();
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
    println!("Found files {:?}", paths.len());
    for path in paths {
        let res: Result<CollectedMetadata> = get_metadata_nom(&path);
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

        println!("Adding: {}", entry);

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
                    println!("Skipping {path_buf:?}");
                }
            }
        }
    } else if dir.is_file() {
        paths.push(dir);
    } else {
        println!("unknown {dir:?}");
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

fn get_file_metadata(filename: &PathBuf) -> Result<FileMetadata> {
    if !path_exists(filename.clone()) {
        anyhow::bail!("File doesn't exist");
    }

    let extension = filename
        .extension()
        .unwrap()
        .to_os_string()
        .into_string()
        .unwrap()
        .to_lowercase();
    let base_file_name = filename
        .file_name()
        .ok_or(Error::msg("File metadata read error"))?
        .to_os_string()
        .into_string()
        .unwrap();
    let file_size = filename.metadata()?.size();
    let creation_date = filename
        .metadata()?
        .created()
        .ok()
        .map(|t| format!("{:?}", t));

    Ok(FileMetadata {
        extension,
        base_file_name,
        file_size,
        creation_date,
    })
}

fn get_image_metadata(filename: &PathBuf) -> Result<ImageMetadata> {
    if !path_exists(filename.clone()) {
        anyhow::bail!("File doesn't exist");
    }

    let mut image_meta = ImageMetadata::default();

    assert!(!is_video(&filename));
    let meta = Metadata::new_from_path(filename)?;
    image_meta.date = meta.get_tag_string("Exif.Photo.DateTimeOriginal")?;
    let xres = meta.get_tag_rational("Exif.Photo.PixelXDimension");
    let yres = meta.get_tag_rational("Exif.Photo.PixelYDimension");
    if xres.is_some() && yres.is_some() {
        image_meta.resolution = Some((xres.unwrap(), yres.unwrap()));
    } else {
        // for tag in meta.get_exif_tags().unwrap().iter().filter(|f| !f.contains("Sony") && !f.contains("Note")) {
        //     println!("tag: {:?} val: {:?}", tag, meta.get_tag_interpreted_string(tag.as_str()));
        // }
    }
    image_meta.model = meta.get_tag_string("Exif.Image.Model").ok();
    image_meta.brightness = meta.get_tag_string("Exif.Photo.BrightnessValue").ok();
    Ok(image_meta)
}

fn get_video_metadata(filename: &PathBuf) -> Result<VideoMetadata> {
    if !path_exists(filename.clone()) {
        anyhow::bail!("File doesn't exist");
    }

    let mut video_meta = VideoMetadata::default();

    let mut parser = MediaParser::new();
    let ms = MediaSource::file_path(filename)?;
    assert!(ms.has_track());
    let track_info: TrackInfo = parser.parse(ms)?;
    video_meta.video_duration = track_info
        .get(TrackInfoTag::DurationMs)
        .map(|f| Duration::from_millis(f.as_u64().unwrap()));
    video_meta.date = track_info
        .get(TrackInfoTag::CreateDate)
        .map(|f| f.as_time().unwrap().to_rfc3339())
        .unwrap();

    return Ok(video_meta);
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

fn get_metadata_nom(filename: &PathBuf) -> Result<CollectedMetadata> {
    let file_metadata = get_file_metadata(filename)?;
    let image_metadata;
    let video_metadata;

    // println!("file: {:?}", filename);
    if file_metadata.extension == "mp4" {
        image_metadata = None;
        video_metadata = Some(get_mp4_metadata(filename)?);
    } else if VIDEOS_EXTENSIONS.contains(&file_metadata.extension.as_str()) {
        image_metadata = None;
        video_metadata = Some(get_video_metadata(filename)?);
    } else {
        image_metadata = Some(get_image_metadata(filename)?);
        video_metadata = None;
    };

    Ok(CollectedMetadata {
        file_metadata,
        image_metadata,
        video_metadata,
    })
}

fn get_mp4_metadata(filename: &PathBuf) -> Result<VideoMetadata> {
    let f = File::open(filename)?;
    let size = f.metadata()?.len();
    let reader = BufReader::new(f);
    let mp4 = mp4::Mp4Reader::read_header(reader, size)?;

    if mp4.moov.mvhd.creation_time == 0 {
        bail!("no creation time");
    }
    let timestamp = if mp4.moov.mvhd.creation_time > MP4_TO_UNIX_OFFSET {
        mp4.moov.mvhd.creation_time - MP4_TO_UNIX_OFFSET
    } else {
        mp4.moov.mvhd.creation_time
    };
    let dt = DateTime::from_timestamp(timestamp.try_into().unwrap(), 0).expect("invalid timestamp");
    Ok(VideoMetadata {
        date: dt.to_string(),
        video_duration: Some(mp4.duration()),
    })
}
