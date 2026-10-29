//! The parts of a file a reader has seen. Readers never do IO: when they need bytes that aren't there, they say which
//! with [`Missing`], the caller reads them and runs the reader again. The desktop tests hand over whole files; the
//! browser reads slices of a `File` (REQ-019-04).

/// Bytes at `at..at + len` are needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Missing {
    pub at: u64,
    pub len: u64,
}

/// Known ranges of a file of `len` bytes.
#[derive(Debug, Clone, Default)]
pub struct Bytes {
    len: u64,
    ranges: Vec<(u64, Vec<u8>)>,
}

impl Bytes {
    /// Nothing known yet of a file of `len` bytes.
    pub fn new(len: u64) -> Self {
        Self {
            len,
            ranges: Vec::new(),
        }
    }

    /// A whole file.
    pub fn whole(data: Vec<u8>) -> Self {
        Self {
            len: data.len() as u64,
            ranges: vec![(0, data)],
        }
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bytes read so far.
    pub fn known(&self) -> u64 {
        self.ranges.iter().map(|(_, data)| data.len() as u64).sum()
    }

    /// Adds bytes read at `at`.
    pub fn add(&mut self, at: u64, data: Vec<u8>) {
        if !data.is_empty() {
            self.ranges.push((at, data));
        }
    }

    /// `len` bytes at `at`, cut at the end of the file. [`Missing`] if they haven't been read.
    pub fn get(&self, at: u64, len: u64) -> Result<&[u8], Missing> {
        let len = len.min(self.len.saturating_sub(at));
        for (start, data) in &self.ranges {
            let end = start + data.len() as u64;
            if *start <= at && at + len <= end {
                let from = (at - start) as usize;
                return Ok(&data[from..from + len as usize]);
            }
        }
        Err(Missing { at, len })
    }

    /// Exactly `len` bytes at `at`, or `None` if the file is shorter (then the structure is broken, not missing).
    pub fn exact(&self, at: u64, len: u64) -> Result<Option<&[u8]>, Missing> {
        if at.checked_add(len).is_none_or(|end| end > self.len) {
            return Ok(None);
        }
        self.get(at, len).map(Some)
    }
}

pub(crate) fn u16_be(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn u32_be(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

pub(crate) fn u64_be(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

pub(crate) fn u16_le(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn u32_le(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_what_is_missing_and_cuts_at_the_end() {
        let mut bytes = Bytes::new(10);
        assert_eq!(bytes.get(2, 3), Err(Missing { at: 2, len: 3 }));
        bytes.add(0, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(bytes.get(2, 3), Ok(&[2, 3, 4][..]));
        assert_eq!(bytes.get(8, 5), Err(Missing { at: 8, len: 2 }));
        bytes.add(6, vec![6, 7, 8, 9]);
        assert_eq!(bytes.get(8, 5), Ok(&[8, 9][..]));
        assert_eq!(bytes.exact(8, 5), Ok(None));
    }
}
