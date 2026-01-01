#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]
#![allow(clippy::multiple_crate_versions)]

pub mod asana;
pub mod cache;
pub mod commands;
pub mod config;
pub mod context;
pub mod focus;
pub mod task;
