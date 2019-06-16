use bytes::{Buf, BufMut};
use std::io::Error;
use std::io::Read;

#[derive(Clone, Debug)]
pub struct ByteBuf {
    inner: Vec<u8>,
    read_cursor_position: usize,

    marked_read_position: usize,
}

impl ByteBuf {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
            read_cursor_position: 0,

            marked_read_position: 0,
        }
    }

    pub fn new() -> Self {
        Self {
            inner: vec![],
            read_cursor_position: 0,

            marked_read_position: 0,
        }
    }

    pub fn read_position(&self) -> usize {
        self.read_cursor_position
    }


    pub fn mark_read_position(&mut self) {
        self.marked_read_position = self.read_cursor_position;
    }

    pub fn reset_read_position(&mut self) {
        self.read_cursor_position = self.marked_read_position;
    }

    pub fn inner(&self) -> &[u8] {
        &self.inner[self.read_cursor_position..]
    }

    pub unsafe fn inner_mut(&mut self) -> &mut [u8] {
        let len = self.inner.len();
        &mut self.inner[len..]
    }

    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }

    /// Removes all bytes prior to the current read position.
    pub fn remove_prior(&mut self) {
        let new_capacity = self.inner.capacity() - self.read_cursor_position;
        let mut new_inner = Vec::with_capacity(new_capacity);

        new_inner.extend_from_slice(&self.inner);

        self.inner = new_inner;
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn capacity(&self) -> usize { self.inner.capacity() }
}

impl Buf for ByteBuf {
    fn remaining(&self) -> usize {
        trace!("remaining {}", self.inner.len() - self.read_cursor_position);
        self.inner.len() - self.read_cursor_position
    }

    fn bytes(&self) -> &[u8] {
        unsafe {
            &std::slice::from_raw_parts(self.inner.as_ptr(), self.inner.capacity())[self.read_cursor_position..]
        }
    }

    fn advance(&mut self, cnt: usize) {
        self.read_cursor_position += cnt;
    }
}

impl BufMut for ByteBuf {
    fn remaining_mut(&self) -> usize {
        trace!("remaining_mut {}", self.inner.capacity() - self.inner.len());
        self.inner.capacity() - self.inner.len()
    }

    unsafe fn advance_mut(&mut self, cnt: usize) {
        self.inner.set_len(self.inner.len() + cnt);
    }

    unsafe fn bytes_mut(&mut self) -> &mut [u8] {
        let r = &mut std::slice::from_raw_parts_mut(self.inner.as_mut_ptr(), self.inner.capacity())[self.inner.len()..];
        r
    }
}

impl Read for ByteBuf {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let mut amount_read = 0;

        while amount_read < buf.len() {
            let self_index = self.read_cursor_position + amount_read;
            if let Some(val) = self.inner.get(self_index) {
                buf[amount_read] = val.clone();
            } else {
                break;
            }

            amount_read += 1;
        }

        Ok(amount_read)
    }
}
