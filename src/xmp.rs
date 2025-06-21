use anyhow::{Error, Result};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::str::FromStr;
use xmp_toolkit::{XmpMeta, xmp_ns};

struct CircularBuffer<T> {
    buffer: Vec<T>,
    index: usize,
}

impl<T: Default + Clone + Copy> CircularBuffer<T> {
    fn new(size: usize) -> CircularBuffer<T> {
        CircularBuffer {
            buffer: vec![Default::default(); size],
            index: 0,
        }
    }

    fn push(&mut self, value: T) {
        self.buffer[self.index] = value;
        self.index = (self.index + 1) % self.buffer.len();
    }

    fn get(&self, ith: usize) -> T {
        if ith >= self.buffer.len() {
            panic!("Index out of bounds");
        }
        self.buffer[(self.index + ith) % self.buffer.len()]
    }

    fn iter(&self) -> CircularBufferIterator<T> {
        CircularBufferIterator::<T> {
            buffer: self,
            current_index: 0,
        }
    }
}

struct CircularBufferIterator<'a, T> {
    buffer: &'a CircularBuffer<T>,
    current_index: usize,
}

impl<T: Copy + Default> Iterator for CircularBufferIterator<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_index < self.buffer.buffer.len() {
            let value = self.buffer.get(self.current_index);
            self.current_index += 1;
            Some(value)
        } else {
            None
        }
    }
}

impl<T: Default + Clone + Copy + PartialEq> CircularBuffer<T> {
    fn contains(&self, subarray: &[T]) -> bool {
        if subarray.len() > self.buffer.len() {
            return false;
        }

        for (i, item) in self.iter().enumerate() {
            if item != subarray[i] {
                return false;
            }
        }

        true
    }
}

const XMP_START: &[u8] = b"<x:xmpmeta";
const XMP_END: &[u8] = b"</x:xmpmeta>";
const XMP_SEARCH_BUFFER_SIZE: usize = 1024 * 128;
const XMP_END_SEARCH_SPACE_SIZE: usize = 1024 * 1024 * 1;
const XMP_MAX_SEARCH_SPACE_SIZE: usize = 1024 * 1024 * 1;

pub fn read_rating_xmp(filename: PathBuf) -> Result<i32> {
    let xmp_data = extract_xmp_data(filename.clone(), true)?
        .or_else(|| extract_xmp_data(filename, false).unwrap());

    if xmp_data.is_none() {
        anyhow::bail!("XMP data not found in the file.");
    }

    let xmp_meta = XmpMeta::from_str(std::str::from_utf8(&xmp_data.as_ref().unwrap()).unwrap());

    Ok(xmp_meta
        .unwrap()
        .property(xmp_ns::XMP, "Rating")
        .map_or(0, |prop| prop.value.parse::<i32>().unwrap()))
}

pub fn read_timestamp_xmp(filename: PathBuf) -> Result<String> {
    let xmp_data = extract_xmp_data(filename.clone(), true)?
        .or_else(|| extract_xmp_data(filename, false).unwrap());

    if xmp_data.is_none() {
        anyhow::bail!("XMP data not found in the file.");
    }

    let xmp_meta = XmpMeta::from_str(std::str::from_utf8(&xmp_data.as_ref().unwrap()).unwrap());

    xmp_meta.unwrap().property(xmp_ns::XMP, "CreateDate").map(|f| f.value).ok_or(Error::msg("Timestamp not found"))
}

fn extract_xmp_data(
    filename: PathBuf,
    read_from_end_of_file: bool,
) -> Result<Option<Vec<u8>>, Error> {
    let file = File::open(filename).unwrap();
    let mut reader = BufReader::new(file);
    let mut buffer = vec![0; XMP_SEARCH_BUFFER_SIZE];
    let mut total_bytes_read = 0;
    let mut start_buffer = CircularBuffer::new(XMP_START.len());
    let mut end_buffer = CircularBuffer::new(XMP_END.len());
    let mut start_found = false;
    let mut xmp_data = XMP_START.to_vec();

    if read_from_end_of_file {
        reader.seek(SeekFrom::End(-(XMP_END_SEARCH_SPACE_SIZE as i64)))?;
    }

    while let Ok(n) = reader.read(&mut buffer) {
        if n == 0 {
            break;
        }

        for &byte in buffer.iter().take(n) {
            if !start_found {
                start_buffer.push(byte);
            } else {
                end_buffer.push(byte);
            }
            total_bytes_read += 1;

            if total_bytes_read > XMP_MAX_SEARCH_SPACE_SIZE {
                return Ok(None);
            }

            if !start_found && start_buffer.contains(XMP_START) {
                start_found = true;
            }

            if start_found {
                xmp_data.push(byte);
            }

            if start_found && end_buffer.contains(XMP_END) {
                return Ok(Some(xmp_data));
            }
        }
    }
    Ok(None)
}

// fn extract_rating_from_xmp_data(xmp_data: &Vec<u8>) -> Result<Option<i32>> {
//     let xmp_data = String::from_utf8_lossy(xmp_data);
//     for line in xmp_data.lines() {
//         if line.contains("xmp:Rating") {
//             let rating_str = if line.contains("=") {
//                 line.to_string()
//                     .split('=')
//                     .nth(1)
//                     .unwrap()
//                     .chars()
//                     .filter(char::is_ascii_digit)
//                     .collect::<String>()
//             } else {
//                 line.to_string()
//                     .split('>')
//                     .nth(1)
//                     .unwrap()
//                     .split('<')
//                     .nth(0)
//                     .unwrap()
//                     .chars()
//                     .filter(char::is_ascii_digit)
//                     .collect::<String>()
//             };
//             return Ok(Some(rating_str.parse::<i32>().unwrap()));
//         }
//     }
//     return Ok(None);
// }
