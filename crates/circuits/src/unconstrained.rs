// Copyright 2024 Ulvetanna Inc.
use crate::builder::ConstraintSystemBuilder;
use binius_core::oracle::OracleId;
use binius_field::{
	as_packed_field::{PackScalar, PackedType},
	underlier::UnderlierType,
	BinaryField1b, PackedField, TowerField,
};
use bytemuck::{must_cast_slice_mut, Pod};
use rand::{thread_rng, Rng};
use rayon::prelude::*;

pub fn unconstrained<U, F>(
	builder: &mut ConstraintSystemBuilder<U, F>,
	log_size: usize,
) -> Result<OracleId, anyhow::Error>
where
	U: UnderlierType + Pod + PackScalar<F> + PackScalar<BinaryField1b>,
	F: TowerField,
{
	let rng = builder.add_committed(log_size, BinaryField1b::TOWER_LEVEL);

	if let Some(witness) = builder.witness() {
		let len = 1 << (log_size - <PackedType<U, BinaryField1b>>::LOG_WIDTH);
		let mut data = vec![U::default(); len].into_boxed_slice();
		must_cast_slice_mut::<_, u8>(&mut data)
			.into_par_iter()
			.for_each_init(thread_rng, |rng, data| {
				*data = rng.gen();
			});
		*witness = std::mem::take(witness)
			.update_owned::<BinaryField1b, Box<[U]>>([(rng, data)].into_iter())?;
	}

	Ok(rng)
}
