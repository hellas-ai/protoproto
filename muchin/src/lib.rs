#![feature(generic_const_exprs)]
#![allow(incomplete_features)]

pub mod automaton;
pub mod models;

pub extern crate gensym;
pub extern crate linkme;
pub extern crate paste;

#[cfg(test)]
pub mod tests;
