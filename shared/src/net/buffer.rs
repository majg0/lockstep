use std::mem::{align_of, size_of, MaybeUninit};

use crate::endian::Endian;

#[derive(Clone, Debug)]
pub struct Buffer {
    data: Vec<u8>,
    pub index: usize,
}

impl Buffer {
    pub fn with_capacity(capacity: usize) -> Self {
        Buffer {
            data: vec![0; capacity],
            index: 0,
        }
    }

    pub fn write_at<T: Endian + Copy>(&mut self, value_ne: T, index: usize) {
        let size = size_of::<T>();
        assert!(self.data.len() >= index + size, "write_at out of bounds");

        let value_le = value_ne.to_le();
        let value_le_ptr = &value_le as *const T as *const u8;
        unsafe {
            let buffer_ptr = self.data.as_mut_ptr().add(index);

            {
                let alignment = align_of::<T>();
                assert!(
                    (value_le_ptr as usize) % alignment == 0,
                    "Source is not properly aligned"
                );
                assert!(
                    (buffer_ptr as usize) % alignment == 0,
                    "Destination (buffer pointer) is not properly aligned"
                );
            }

            std::ptr::copy_nonoverlapping(value_le_ptr, buffer_ptr, size);
        }
    }

    pub fn write<T: Endian + Copy>(&mut self, value_ne: T) {
        self.write_at(value_ne, self.index);
        self.index += size_of::<T>();
    }

    pub fn pad<T>(&mut self) {
        let size = size_of::<T>();
        self.index += size;
    }

    pub fn peek<T: Endian + Copy>(&mut self) -> T {
        let size = size_of::<T>();
        assert!(
            self.data.len() >= self.index + size,
            "read size {} >= index {} + size {}",
            self.data.len(),
            self.index,
            size
        );

        let mut value_le = MaybeUninit::<T>::uninit();
        let value_le_ptr = value_le.as_mut_ptr() as *mut u8;

        unsafe {
            let buffer_ptr = self.data.as_ptr().add(self.index);

            {
                let alignment = align_of::<T>();
                assert!(
                    (value_le_ptr as usize) % alignment == 0,
                    "Destination is not properly aligned"
                );
                assert!(
                    (buffer_ptr as usize) % alignment == 0,
                    "Source (buffer pointer) is not properly aligned"
                );
            }

            std::ptr::copy_nonoverlapping(buffer_ptr, value_le_ptr, size);
        }

        unsafe { value_le.assume_init() }.to_ne()
    }

    pub fn read<T: Endian + Copy>(&mut self) -> T {
        let result = self.peek();
        self.index += size_of::<T>();
        result
    }

    pub fn full_slice_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.data.as_mut_ptr(), self.data.capacity()) }
    }

    pub fn written_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.data.as_ptr(), self.index) }
    }

    pub fn read_slice(&self) -> &[u8] {
        &self.data // NOTE: uses len set by reset_reader
    }

    pub fn reset_reader(&mut self, eof: usize) {
        assert!(eof <= self.data.capacity());
        unsafe { self.data.set_len(eof) };
        self.index = 0;
    }

    pub fn read_size(&self) -> usize {
        self.data.len()
    }

    pub fn written_size(&self) -> usize {
        self.index
    }

    pub fn reset_writer(&mut self) {
        unsafe { self.data.set_len(self.data.capacity()) };
        self.index = 0;
    }
}
