//! Flashmob studies load flow, short circuit, protective-device coordination,
//! and IEEE 1584-2018 arc flash. The same engine feeds the macOS window and
//! the headless JSON command line.

pub mod arcflash;
pub mod curves;
pub mod exec;
pub mod fault;
pub mod loadflow;
pub mod model;
pub mod skm;
pub mod sld;
pub mod study;
pub mod tcc;

mod cplx;
mod linalg;
mod network;
mod solve;
