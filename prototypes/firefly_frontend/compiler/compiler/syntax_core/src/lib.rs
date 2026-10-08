#![deny(warnings)]
#![feature(iterator_try_collect)]
#![feature(box_patterns)]

mod ir;
pub mod macros;
pub mod passes;
pub mod printer;

pub use self::ir::*;
