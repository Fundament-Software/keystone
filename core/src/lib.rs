mod binary_embed;
mod buffer_allocator;
mod byte_stream;
mod cap_replacement;
mod cap_std_capnproto;
mod cell;
mod config;
mod database;
pub mod host;
pub mod http;
pub mod keystone;
mod posix_module;
mod posix_spawn;
mod proxy;
mod spawn;
mod sqlite;

capnp_import::capnp_import!("schema/**/*.capnp");

#[cfg(test)]
capnp_import::capnp_import!("../modules/hello-world/*.capnp");

#[cfg(test)]
capnp_import::capnp_import!("../modules/stateful/*.capnp");

#[cfg(test)]
capnp_import::capnp_import!("../modules/config-test/*.capnp");

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
