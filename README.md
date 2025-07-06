## `rcc`

Media management utility designed to detect duplicate files based on metadata rather than exact bitwise match.

## Description

`rcc` works by first scanning the provided directories and extracting metadata from photos and videos. Then it compares the metadata between each entry to find duplicates and generates a shell script to run the selected action.

For example the following command:
```
rcc -o "photosync.sh" -c delete --src "/media/PhotoSync/" --dest "/media/Images/"
```
would generate a `photosync.sh` that would contain commands to `rm` every file from `/media/PhotoSync` that already exists in `/media/Images`.

This approach is useful for cases where the files do not match perfectly in contents due to modified metadata, such as setting the `Rating` exif tag.

## Usage

The difference between `SRC` and `DEST` directories is that in terms of removal the files are supposed to be removed from the source, rather than from the destination.

```
Usage: rcc [OPTIONS] --dest <DEST> --src <SRC> [MODE]

Arguments:
  [MODE]  [default: paranoid] [possible values: loose, paranoid]

Options:
  -v, --verbose
  -e, --exclude <EXCLUDE>
  -f, --flip-exclusion
  -a, --include-videos
  -o, --output <OUTPUT>    [default: run.sh]
  -c, --command <COMMAND>  [possible values: move, copy, delete, print]
  -d, --dest <DEST>
  -s, --src <SRC>
  -h, --help               Print help
```
