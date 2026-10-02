//! UI regressions organized by user behavior.
mod canvas;
mod concurrency;
mod fixtures;
mod gestures;
mod hover;
mod input;
mod layout;
mod lifecycle;
mod navigation;
mod project;
mod scene_work;
mod variables;

use super::render::project_settings_label;
use super::shaping::code_connections;
use super::shaping::{card_title, variable_highlight_spans};
use super::*;
use fixtures::*;
use gpui::{
    AppContext, Context, Modifiers, MouseButton, MouseMoveEvent, ScrollDelta, ScrollWheelEvent,
    TestAppContext, VisualContext, Window, point, px,
};
use refscape_application::{
    ApplicationSnapshot, Command, ImportedSession, PersistableSession, SessionRepository, ViewEvent,
};
use refscape_model::{ConnectionKind, Position, ProjectLanguage, Viewport};
use refscape_model::{SourceDocument, SourceRange};
use std::path::Path;
use std::sync::{Arc, Mutex};
