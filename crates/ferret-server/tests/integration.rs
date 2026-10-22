//! The server's black-box tests, one module per area. One binary keeps the build from linking
//! the whole server once per area.

mod common;

mod activity;
mod admin;
mod affine_plugin;
mod api_keys;
mod auth;
mod bootstrap;
mod carapace_plugin;
mod documents;
mod endpoints;
mod exclusions;
mod explore;
#[cfg(target_os = "linux")]
mod files_plugin;
mod git_plugin;
mod gitea_plugin;
mod graphql_search;
mod homeassistant_plugin;
mod imap_plugin;
mod inventory;
mod judgments;
mod karakeep_plugin;
mod mcp;
mod mealie_plugin;
mod model_registry;
mod models;
mod moneta_plugin;
mod nullable_dates;
mod paperless_plugin;
mod runs;
mod search;
mod search_metadata;
mod semantic;
mod startup;
mod statistics;
mod syncs;
mod threads;
mod vikunja_plugin;
mod whatsapp_plugin;
mod writer_stats;
mod writers;
