#[repr(C)]
#[derive(PartialEq)]
#[derive(Clone)]
#[derive(Copy)]
#[derive(Debug)]
pub struct PackedPrimitiveType {
    pub value: M128,
}

#[repr(C)]
#[derive(PartialEq)]
#[derive(Clone)]
#[derive(Copy)]
#[derive(Debug)]
pub struct M128 {
    pub high: u64,
    pub low: u64,
}

#[repr(C)]
#[derive(Clone)]
#[derive(Copy)]
#[derive(Debug)]
pub struct ScaledPackedField<PT, const N: usize> {
    pub elements: [PT; N], // Fixed-size array of PT
}

impl<PT: PartialEq, const N: usize> PartialEq for ScaledPackedField<PT, N> {
    fn eq(&self, other: &Self) -> bool {
        self.elements == other.elements
    }
}

// impl PartialEq for ScaledPackedField<PackedPrimitiveType, 2> {
// 	fn eq(&self, other: &Self) -> bool {
//         self.elements == other.elements
// 	}
// }



extern "C" {
    // total_length is the length of the input in bytes
    // chunk_size is the chunk size in bytes
    pub fn binius_groestl_bs_hash(array: *mut ScaledPackedField<PackedPrimitiveType, 2>, array: *mut PackedPrimitiveType, total_length: usize, chunk_size: usize);

}


