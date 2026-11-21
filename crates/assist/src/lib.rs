//! Optional natural-language to LaTeX authoring.
//!
//! This crate is an integration, deliberately outside `core` and outside the plugin sandbox
//! (plugins have no network): the editor calls it only when the user presses **Convert** in the
//! Math dialog, with the text they typed. It speaks one protocol, the OpenAI-compatible
//! `chat/completions` request, which covers `OpenAI`, Gemini's OpenAI-compatible endpoint and
//! local servers such as Ollama or llama.cpp (a fully offline option). Nothing here knows about
//! any provider.
//!
//! * [`AssistSettings`] is the non-secret configuration (base URL and model).
//! * [`Endpoint`] is a validated settings snapshot plus the optional API key, which the caller
//!   reads from the operating system keyring; the key is never serialised, logged or put in an
//!   error message.
//! * [`Converter`] is the seam the editor injects ([`HttpConverter`] in production, a mock in
//!   tests); [`CancelToken`] aborts a request in flight.
//! * [`clean_latex`] turns a model reply into plain LaTeX and rejects replies that are not.

mod client;
mod error;
mod output;
mod prompt;
mod settings;

pub use client::{CancelToken, Converter, HttpConverter};
pub use error::AssistError;
pub use output::clean_latex;
pub use prompt::{MAX_INPUT_CHARS, SYSTEM_PROMPT};
pub use settings::{AssistSettings, Endpoint, PRESETS, Preset, Secret};
