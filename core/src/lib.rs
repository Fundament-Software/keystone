mod binary_embed;
mod buffer_allocator;
mod byte_stream;
mod cap_replacement;
mod cap_std_capnproto;
mod cell;
pub mod config;
mod database;
pub mod host;
pub mod http;
pub mod keystone;
mod posix_module;
mod posix_process;
mod posix_spawn;
pub mod proxy;
mod sqlite;

include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

impl From<keystone_capnp::LogLevel> for tracing::Level {
    fn from(val: keystone_capnp::LogLevel) -> Self {
        match val {
            keystone_capnp::LogLevel::Trace => tracing::Level::TRACE,
            keystone_capnp::LogLevel::Debug => tracing::Level::DEBUG,
            keystone_capnp::LogLevel::Info => tracing::Level::INFO,
            keystone_capnp::LogLevel::Warning => tracing::Level::WARN,
            keystone_capnp::LogLevel::Error => tracing::Level::ERROR,
        }
    }
}
