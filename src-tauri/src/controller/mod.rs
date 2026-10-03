pub mod protocol;

pub mod bootstrap;
pub mod client;
pub mod mobile;

#[cfg(desktop)]
pub mod commands;
#[cfg(desktop)]
pub mod host;
#[cfg(desktop)]
pub mod identity;
#[cfg(desktop)]
pub mod pairing;
#[cfg(desktop)]
pub mod secrets;
