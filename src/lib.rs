//! A compact collection of overlapping strings.
//!
//! [`SubStr`] stores a collection of strings as one big string
//! and a vector with the location and length of each string.
//! During the build process substrings are identified. For example
//! 'substring' can also store the strings 'sub', 'ring' etc. Also
//! strings are combined to create new substring locations: e.g.
//! by placing 'each' after 'with', 'the' can be stored. This can
//! result in significant compression.
//!
//! # Limitations
//!
//! - The resulting collection is immutable
//! - Construction is time consuming
//! - Stores strings with a maximum length of `u8::MAX` **bytes**
//! - Compression dependent on input (might be small)
//! - Access is slower compared to a Vec
//!
//! # Feature flags
//!
//! - `serde`: makes `SubStr` and `Builder` serializable.

use aho_corasick::AhoCorasick;
use derive_more::From;
use std::collections::HashMap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
/// A compact collection of strings.
///
/// Creating the collection is a time consuming process which is done
/// done using [`Builder`]. Once created no new elements can be added
/// or changed. Individual elements can be accessed using `get()` or
/// get an Iterator over the elements using iter().
#[derive(Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SubStr {
    pub spans: Vec<(u32, u8)>,
    pub string: String,
}

impl SubStr {
    /// Returns the number of elements in the substring vector, also referred to as its ‘length’.
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Returns the length of the storage string in bytes.
    pub fn storage_len(&self) -> usize {
        self.string.len()
    }

    /// Returns `true` if the substring vector contains no elements.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Return the `&str` at `index` if the element
    /// exists.
    pub fn get(&self, index: usize) -> Option<&str> {
        (index < self.len()).then_some(
            &self.string[self.spans[index].0 as usize
                ..self.spans[index].0 as usize + self.spans[index].1 as usize],
        )
    }

    /// Returns an iterator over the collection.
    pub fn iter<'a>(&'a self) -> Iter<'a> {
        Iter {
            current_item: 0,
            vec: &self,
        }
    }

    /// Returns part of the storage string immediately in front of the item.
    pub fn before(&self, index: usize, len: usize) -> Option<&str> {
        if let Some((position, _)) = self.spans.get(index) {
            let position = *position as usize;
            let mut start = if position < len { 0 } else { position - len };
            while !self.string.is_char_boundary(start) {
                start -= 1;
            }
            Some(&self.string[start..position])
        } else {
            None
        }
    }

    /// Returns part of the storage string immediately behind of the item.
    pub fn after(&self, index: usize, len: usize) -> Option<&str> {
        if let Some((position, length)) = self.spans.get(index) {
            let position = *position as usize;
            let length = *length as usize;
            let mut end = if self.string.len() <= position + length + len {
                self.string.len()
            } else {
                position + length + len
            };
            while !self.string.is_char_boundary(end) {
                end += 1;
            }
            Some(&self.string[position + length..end])
        } else {
            None
        }
    }
}

pub struct Iter<'a> {
    current_item: usize,
    vec: &'a SubStr,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_item < self.vec.len() {
            self.current_item += 1;
            self.vec.get(self.current_item - 1)
        } else {
            None
        }
    }
}

/// A [`SubStr`] builder.
///
/// You can turn a `Vec<String>` into a `Builder` using `TryFrom`
/// or from something that can be turned into an `Iterator`
/// over  anything that can be turned into a `&str` using [`from_iter()`].
///
/// You can construct a [`SubStr`] or [`SubStrMap`] using the [`build_substr()`]
/// or [`build_substr_map()`] methods. If you want to verify the result
/// use the [`verify()`] method before construction. The build process
/// can take a long time, use [`messages()`] to show progress on stdout.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Builder {
    vec: Vec<String>,
    contained_in: Vec<Option<(u32, u8)>>,
    index_string: String,
    spans: Vec<Option<(u32, u8)>>,
    silent: bool,
    build: bool,
}

impl TryFrom<Vec<String>> for Builder {
    type Error = Error;

    fn try_from(value: Vec<String>) -> Result<Self> {
        let max_len = value.iter().map(|s| s.len()).max().unwrap();
        if max_len > u8::MAX as usize {
            return Err(Error::StringTooLong(max_len));
        }
        Ok(Self {
            contained_in: vec![None; value.len()],
            spans: vec![None; value.len()],
            index_string: String::new(),
            silent: true,
            build: false,
            vec: value,
        })
    }
}

impl Builder {
    /// Create a `SubStr` `Builder` from an Iterator. This
    /// method checks the length of the strings added, and fails
    /// when it it contains a string that is to long. (This
    /// is why `Builder` doesn't implement `FromIterator` )
    pub fn from_iter<I, S>(iter: I) -> Result<Builder>
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        let vec: Vec<String> = iter.into_iter().map(|s| s.as_ref().to_string()).collect();

        let max_len = vec
            .iter()
            .map(|s| s.len())
            .max()
            .ok_or(Error::NoMaxStringLen)?;
        if max_len > u8::MAX as usize {
            return Err(crate::Error::StringTooLong(max_len));
        }

        Ok(Self {
            contained_in: vec![None; vec.len()],
            spans: vec![None; vec.len()],
            index_string: String::new(),
            silent: true,
            build: false,
            vec,
        })
    }

    pub fn debug_messages(&mut self, on: bool) {
        self.silent = !on;
    }

    pub fn build_only(&mut self) -> Result<()> {
        if self.build {
            return Ok(());
        }

        if !self.silent {
            println!("1/4 -> Looking for substrings ...");
        }
        self.find_substrings();

        if !self.silent {
            println!("2/4 -> Looking for partial substrings ...");
        }
        self.find_partial_substrings();

        if !self.silent {
            println!("3/4 -> Adding uncontained strings ...");
        }
        self.join_loose_strings();

        if !self.silent {
            println!("4/4 -> Adding substrings ...");
        }
        self.join_substrings();
        if !self.silent {
            println!("    -> Finished");
        }
        self.build = true;
        Ok(())
    }

    pub fn build(mut self) -> Result<SubStr> {
        if !self.build {
            self.build_only()?;
        }
        Ok(SubStr {
            string: self.index_string,
            spans: self.spans.iter().map(|s| s.unwrap()).collect(),
        })
    }

    /// Check if the build process was successful.
    pub fn verify(&mut self) -> Result<bool> {
        if !self.build {
            self.build_only()?;
        }
        for (i, w) in self.vec.iter().enumerate() {
            if let Some((b, e)) = self.spans[i] {
                if w != &self.index_string[b as usize..(b as usize + e as usize)] {
                    self.debug(i);
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    // 1/4 of building
    // Find strings that are substrings of other strings.
    fn find_substrings(&mut self) {
        let ac = AhoCorasick::new(self.vec.iter().map(String::as_str)).unwrap();
        for (i, w) in self.vec.iter().enumerate() {
            for mat in ac.find_overlapping_iter(&w) {
                let index = mat.pattern().as_usize();
                let start = mat.start() as u8;
                if index != i && self.contained_in[index].is_none() {
                    self.contained_in[index] = Some((i as u32, start));
                }
            }
        }
    }

    // 1/4 of building
    // find strings that match the end of the storage string
    fn find_partial_substrings(&mut self) {
        if !self.silent {
            println!("    -> make hashmap ...");
        }

        let mut beginnings: HashMap<String, Vec<u32>> = HashMap::new();
        for (index, string) in self
            .vec
            .iter()
            .enumerate()
            .filter(|(i, _)| self.contained_in[*i].is_none())
        {
            for split_point in 1..string.len() {
                if let Some((begin, _)) = split_after_char(string, split_point) {
                    beginnings
                        .entry(begin.to_string())
                        .or_insert(Vec::new())
                        .push(index as u32);
                }
            }
        }

        if !self.silent {
            println!("    -> adding partial substrings ...");
        }

        let mut position: usize = 0;
        let mut index;
        for (i, string) in self
            .vec
            .iter()
            .enumerate()
            .filter(|(i, _)| self.contained_in[*i].is_none())
        {
            index = i as usize;
            if self.spans[index].is_some() {
                continue;
            }
            self.spans[index] = Some((position as u32, string.len() as u8));
            self.index_string.push_str(string);
            position += string.len();
            while let Some(next) = self.find_next_string(index, position, &beginnings) {
                self.spans[next.index] =
                    Some((next.position as u32, self.vec[next.index].len() as u8));
                self.index_string.push_str(&next.tail);
                index = next.index;
                position = next.position + self.vec[next.index].len();
            }
        }
    }

    fn find_next_string(
        &self,
        index: usize,
        position: usize,
        beginnings: &HashMap<String, Vec<u32>>,
    ) -> Option<NextStr> {
        for split_point in 1..self.vec[index].len() {
            if let Some((_, end)) = split_after_char(&self.vec[index], split_point as usize) {
                if let Some(indices) = beginnings.get(end) {
                    for next_index in indices {
                        let next_index = *next_index as usize;
                        if self.spans[next_index].is_none() {
                            let tail = self.vec[next_index].strip_prefix(end).unwrap().to_string();
                            return Some(NextStr {
                                index: next_index,
                                position: position - end.len(),
                                tail,
                            });
                        }
                    }
                }
            }
        }
        None
    }

    // 3/4 of building
    // Add the strings that are no substrings
    fn join_loose_strings(&mut self) {
        for (i, w) in self.vec.iter_mut().enumerate() {
            if self.contained_in[i].is_none() && self.spans[i].is_none() {
                self.spans[i] = Some((self.index_string.len() as u32, w.len() as u8));
                self.index_string.push_str(w);
            }
        }
    }

    // 4/4 of building
    // Add the substrings.
    fn join_substrings(&mut self) {
        while self.spans.iter().filter(|s| s.is_none()).count() > 0 {
            for (i, (cid, start)) in self.contained_in.iter().enumerate().filter_map(|(i, o)| {
                if o.is_some() {
                    Some((i, o.unwrap()))
                } else {
                    None
                }
            }) {
                if self.spans[i].is_none() {
                    if let Some((container_pos, _)) = self.spans[cid as usize] {
                        self.spans[i] =
                            Some((container_pos + (start as u32), self.vec[i].len() as u8));
                    }
                }
            }
        }
    }

    fn debug(&self, id: usize) {
        print!("{} [{}]", self.vec[id], id);
        if let Some((s, l)) = self.spans[id] {
            let s = s as usize;
            let l = l as usize;
            let mut bss = if s < 10 { 0 } else { s - 10 };
            let mut ess = if self.index_string.len() <= s + l + 10 {
                self.index_string.len()
            } else {
                s + l + 10
            };
            while !self.index_string.is_char_boundary(bss) {
                bss -= 1;
            }
            while !self.index_string.is_char_boundary(ess) {
                ess += 1;
            }
            print!(
                " len: {}  substr: {} -> ...{}({}){}...",
                l,
                s,
                &self.index_string[bss..s],
                &self.index_string[s..s + l],
                &self.index_string[s + l..ess]
            );
        }
        println!("");
        if self.contained_in[id].is_some() {
            println!("    - is contained");
        }
    }
}

struct NextStr {
    index: usize,
    position: usize,
    tail: String,
}

fn split_after_char(s: &str, after: usize) -> Option<(&str, &str)> {
    if after == 0 {
        return None;
    }
    match s.char_indices().skip(after).next() {
        Some((i, _)) => s.split_at_checked(i),
        _ => None,
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    StringTooLong(usize),

    NoMaxStringLen,

    #[from]
    Io(std::io::Error),

    #[from]
    Utf8Error(std::string::FromUtf8Error),

    #[from]
    TryFromSliceError(std::array::TryFromSliceError),
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(fmt, "{self:?}")
    }
}

impl core::error::Error for Error {}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn it_works() {
//         let result = add(2, 2);
//         assert_eq!(result, 4);
//     }
// }
