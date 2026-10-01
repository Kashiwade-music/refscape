//! UI regressions organized by user behavior.
mod canvas;
mod concurrency;
mod fixtures;
mod hover;
mod layout;
mod lifecycle;
mod navigation;
mod project;
mod variables;

use super::render::project_settings_label;
use super::shaping::{card_title, variable_highlight_spans};
use super::*;
use fixtures::*;
use gpui::{Modifiers, TestAppContext, VisualContext};
use refscape_model::{SourceDocument, SourceRange};
use std::path::Path;
