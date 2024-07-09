// Copyright 2024 Ulvetanna Inc.

use cfg_if::cfg_if;

cfg_if! {
	if #[cfg(all(target_arch = "x86_64",target_feature = "avx512bw",target_feature = "avx512vbmi",target_feature = "avx512f",target_feature = "gfni",))] {

		mod groestl_avx512;
		pub use groestl_avx512::Groestl256Core;
	} else {
		mod groestl_table;
		mod portable;
		pub use portable::Groestl256Core;
	}
}
