// Copyright 2024 Ulvetanna Inc.

use std::sync::Arc;

use binius_core::oracle::{OracleId, ShiftVariant};
use binius_field::{
	as_packed_field::{PackScalar, PackedType},
	underlier::{UnderlierType, WithUnderlier},
	BinaryField1b, BinaryField32b, ExtensionField, PackedField, TowerField,
};
use binius_macros::composition_poly;
use bytemuck::{must_cast_slice_mut, Pod};
use rand::{thread_rng, Rng};
use rayon::prelude::*;

use crate::{builder::ConstraintSystemBuilder, step_down::step_down, u32add::u32add};

pub fn u32fib<U, F>(
	builder: &mut ConstraintSystemBuilder<U, F>,
	log_size: usize,
) -> Result<OracleId, anyhow::Error>
where
	U: UnderlierType + Pod + PackScalar<F> + PackScalar<BinaryField1b> + PackScalar<BinaryField32b>,
	F: TowerField + ExtensionField<BinaryField32b>,
{
	let current = builder.add_committed(log_size, BinaryField1b::TOWER_LEVEL);
	let next = builder.add_shifted(current, 32, log_size, ShiftVariant::LogicalRight)?;
	let next_next = builder.add_shifted(current, 64, log_size, ShiftVariant::LogicalRight)?;

	if let Some(witness) = builder.witness() {
		let len = 1 << (log_size - <PackedType<U, BinaryField1b>>::LOG_WIDTH);
		let mut current_witness = vec![U::default(); len].into_boxed_slice();
		let mut next_witness = vec![U::default(); len].into_boxed_slice();
		let mut next_next_witness = vec![U::default(); len].into_boxed_slice();
		{
			let current = must_cast_slice_mut::<_, u32>(&mut current_witness);
			let mut rng = thread_rng();
			current[0] = rng.gen();
			current[1] = rng.gen();
			for i in 2..current.len() {
				current[i] = rng.gen();
				(current[i], _) = current[i - 1].overflowing_add(current[i - 2]);
			}
			(must_cast_slice_mut::<_, u32>(&mut next_witness), &current[1..])
				.into_par_iter()
				.for_each(|(next, current)| {
					*next = *current;
				});
			(must_cast_slice_mut::<_, u32>(&mut next_next_witness), &current[2..])
				.into_par_iter()
				.for_each(|(next_next, current)| {
					*next_next = *current;
				});
		}
		*witness = std::mem::take(witness).update_owned::<BinaryField1b, Arc<[U]>>(
			[
				(current, current_witness.into()),
				(next, next_witness.into()),
				(next_next, next_next_witness.into()),
			]
			.into_iter(),
		)?;
	}

	let packed_log_size = log_size - 5;
	let enabled = step_down(builder, packed_log_size, (1 << packed_log_size) - 2)?;
	let sum = u32add(builder, log_size, current, next)?;
	let sum_packed = builder.add_packed(sum, 5)?;
	let next_next_packed = builder.add_packed(next_next, 5)?;

	if let Some(witness) = builder.witness() {
		let next_next_packed_witness =
			WithUnderlier::to_underliers_ref(witness.get::<BinaryField1b>(next_next)?.evals())
				.iter()
				.cloned()
				.collect();
		let sum_packed_witness =
			WithUnderlier::to_underliers_ref(witness.get::<BinaryField1b>(sum)?.evals())
				.iter()
				.cloned()
				.collect();
		*witness = std::mem::take(witness).update_owned::<BinaryField32b, Arc<[U]>>(
			[
				(next_next_packed, next_next_packed_witness),
				(sum_packed, sum_packed_witness),
			]
			.into_iter(),
		)?;
	}

	builder.assert_zero(
		[sum_packed, next_next_packed, enabled],
		composition_poly!([a, b, enabled] = (a - b) * enabled),
	);

	Ok(current)
}
