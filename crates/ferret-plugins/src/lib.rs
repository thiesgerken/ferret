//! Plugin workers: claim runs from the server's internal API, run the plugin, ingest through the
//! public REST API with the run's token.

pub mod plugins;
pub mod worker;
