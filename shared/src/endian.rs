pub trait Endian {
    fn to_le(self) -> Self;
    fn to_ne(self) -> Self;
}

impl Endian for f64 {
    fn to_le(self) -> Self {
        f64::from_bits(self.to_bits().to_le())
    }
    fn to_ne(self) -> Self {
        f64::from_bits(u64::from_le(self.to_bits()))
    }
}

impl Endian for u8 {
    fn to_le(self) -> Self {
        self
    }
    fn to_ne(self) -> Self {
        self
    }
}

impl Endian for u16 {
    fn to_le(self) -> Self {
        self.to_le()
    }
    fn to_ne(self) -> Self {
        u16::from_le(self)
    }
}

impl Endian for u32 {
    fn to_le(self) -> Self {
        self.to_le()
    }
    fn to_ne(self) -> Self {
        u32::from_le(self)
    }
}
