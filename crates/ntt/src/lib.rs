// Copyright 2024 Ulvetanna Inc.

//! Efficient implementations of the binary field additive NTT.
//!
//! See [LCH14] and [DP24] Section 2.3 for mathematical background.
//!
//! [LCH14]: <https://arxiv.org/abs/1404.3458>
//! [DP24]: <https://eprint.iacr.org/2024/504>

mod additive_ntt;
mod dynamic_dispatch;
mod error;
mod multithreaded;
mod single_threaded;
mod strided_array;
#[cfg(test)]
mod tests;
pub mod twiddle;

pub use additive_ntt::AdditiveNTT;
pub use dynamic_dispatch::{DynamicDispatchNTT, NTTOptions, ThreadingSettings};
pub use error::Error;
pub use multithreaded::MultithreadedNTT;
pub use single_threaded::SingleThreadedNTT;
