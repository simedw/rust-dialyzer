pub mod analyze_scc;
pub mod atomic;
pub mod bindings;
pub mod tree;

#[cfg(test)]
mod tests;

pub mod derived;

pub mod closure_order;

pub mod normalize;

pub mod preparation;

pub mod generation;

pub mod constraint_capture;
pub mod generation_capture_headers;
