// use crate::xmp::read_rating_xmp;
use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use rexiv2::Metadata;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

const IMAGE_EXTENSIONS: [&str; 4] = ["heic", "jpg", "jpeg", "png"];
const VIDEOS_EXTENSIONS: [&str; 3] = ["mov", "mp4", "avi"];

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

    #[arg(short = 'a', long, default_value_t = false)]
    include_videos: bool,

    #[arg(short, long, value_delimiter = ' ', num_args = 1..)]
    src: Vec<std::path::PathBuf>,
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

#[derive(Clone, Eq, PartialEq)]
struct Entry {
    pub path: PathBuf,
    pub timestamp: String
}

fn main() {
    let cli: Cli = Cli::parse();

    rexiv2::initialize().expect("Unable to initialize rexiv2");

    let search_path = cli.src;

    let mut all_paths: Vec<PathBuf> = Vec::new();
    for path in search_path {
        visit_dirs(
            path,
            &mut all_paths,
            0,
            cli.exclude.clone(),
            cli.flip_exclusion,
            cli.include_videos,
            cli.verbose,
        )
        .expect("Failed to iterate over directories");
    }

    let mut all_entries = Vec::new();

    for path in all_paths {
        // let relative_path = path
        //     .strip_prefix(search_path.clone())
        //     .expect(format!("Failed to strip root prefix of file {:?}", path).as_str());

        let res: Result<String> = get_timestamp(path.clone());
        let Ok(timestamp) = res else {
            println!(
                "Skipping {path:?} due to {}",
                res.err().unwrap_or(anyhow!("Unknown error")).to_string()
            );
            continue;
        };

        all_entries.push(
            Entry{
                path,
                timestamp
            }
        )


        // let pass_label_check = if let Some(ref label) = cli.label {
        //     let res: Result<Option<String>, String> = get_label(path.clone());
        //     let Ok(label_res) = res else {
        //         println!(
        //             "Skipping {path:?} due to {}",
        //             res.err().unwrap_or("Unknown error".to_string()).to_string()
        //         );
        //         continue;
        //     };
        //     match label_res {
        //         Some(label_res) => label_res == *label,
        //         None => false,
        //     }
        // } else {
        //     true
        // };

        // let pass_treshold_check = match cli.comparison_command {
        //     ComparisonCommand::MoreEqual => rating >= cli.threshold,
        //     ComparisonCommand::LessEqual => rating <= cli.threshold,
        //     ComparisonCommand::Equal => rating == cli.threshold,
        // };

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

    for entry in all_entries.clone() {
        for compared_entry in &all_entries {
            if entry.timestamp == compared_entry.timestamp {
                println!("Duplicate found for: {:?}\n{:?}", entry.path, compared_entry.path);
            }
        }
    }
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

fn get_timestamp(filename: PathBuf) -> Result<String> {
    if !path_exists(filename.clone()) {
        anyhow::bail!("File doesn't exist");
    }

    // Use xmp-toolkit for video files
    // if is_video(&filename) {
    //     return Ok(read_rating_xmp(filename.clone()).unwrap_or(0));
    // }

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
