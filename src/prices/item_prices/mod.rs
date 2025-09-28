mod impls;
mod types;

pub use types::*;

pub trait Description {
    fn get_description(&self, extras: Vec<String>) -> String;
    fn get_brief_description(&self, extras: Vec<String>) -> String;
}
