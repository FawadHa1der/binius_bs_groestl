#[repr(C)]
#[derive(PartialEq)]
pub struct PackedPrimitiveType {
    value: M128,
}

#[repr(C)]
#[derive(PartialEq)]
pub struct M128 {
    high: u64,
    low: u64,
}

#[repr(C)]

pub struct ScaledPackedField<PT, const N: usize> {
    elements: [PT; N], // Fixed-size array of PT
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



//#[link(name = "testrustinput", kind = "static")]
extern "C" {
    // fn doubler(x: f32) -> f32;
	// int crypto_hash(unsigned char *out, const unsigned char *in, unsigned long long inlen);
    pub fn binius_groestl_bs_hash(array: *mut ScaledPackedField<PackedPrimitiveType, 2>, array: *mut PackedPrimitiveType, total_length: usize, chunk_size: usize);
	// pub fn populate_scaled_packed_fields(array: *mut ScaledPackedField<PackedPrimitiveType, 2>, length: usize);

}
