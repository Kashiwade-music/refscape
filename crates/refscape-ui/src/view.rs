//! Native view state. Business state is read exclusively from the controller.
mod background;
mod hover;
mod input;
mod interaction;
mod layout;
mod navigation;
mod painting;
mod project;
mod render;
mod scene;
mod shaping;
mod sidebar;
#[cfg(test)]
mod tests;
use gpui::{Bounds, Context, FocusHandle, Pixels, ShapedLine, Window};
use interaction::Drag;
use painting::PaintedCard;
use refscape_application::{
    ApplicationController, Command, EffectExecutor, Transition, VariableInspection,
};
use refscape_model::{
    CODE_CARD_HEADER, CODE_LINE_HEIGHT, Point, ProjectOpenOptions, Symbol, Theme,
};
use render::color;
use shaping::connected_word;
use std::{ops::Range, path::PathBuf, sync::Arc};
const HEADER: f32 = CODE_CARD_HEADER;
const LINE: f32 = CODE_LINE_HEIGHT;
pub(crate) struct ExplorerView {
    controller: ApplicationController,
    executor: Arc<dyn EffectExecutor>,
    focus: FocusHandle,
    project: ProjectPresenter,
    search: SearchState,
    pub(crate) canvas: CanvasState,
    hover: HoverState,
    requests: RequestPresenter,
    layout: layout::LayoutState,
    sidebar: sidebar::SidebarCache,
    scene: std::rc::Rc<std::cell::RefCell<scene::SceneCache>>,
    close_ready: bool,
}
struct ProjectPresenter {
    session_path: PathBuf,
    themes: Vec<Theme>,
    files: Vec<PathBuf>,
    symbols: Vec<Symbol>,
    launch_options: ProjectOpenOptions,
}
#[derive(Default)]
struct SearchState {
    query: String,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    bounds: Option<Bounds<Pixels>>,
    line: Option<ShapedLine>,
    focused: bool,
}
#[derive(Default)]
pub(crate) struct CanvasState {
    pub(crate) selected: Option<String>,
    drag: Option<Drag>,
    painted: Vec<PaintedCard>,
    pub(crate) inspection: Option<VariableInspection>,
    selection_generation: u64,
    bounds: Bounds<Pixels>,
    context_hover: Option<(String, usize)>,
    drag_preview: Option<(String, Point)>,
}
#[derive(Default)]
struct HoverState {
    target: Option<hover::HoverTarget>,
    text: Option<String>,
    task: Option<gpui::Task<()>>,
    dismiss_task: Option<gpui::Task<()>>,
    pending_target: Option<hover::HoverTarget>,
    scroll: gpui::ScrollHandle,
}
#[derive(Default)]
struct RequestPresenter {
    busy: bool,
    status: String,
    error: bool,
    closing: bool,
}
pub(crate) struct ViewLaunch {
    pub(crate) session_path: PathBuf,
    pub(crate) themes: Vec<Theme>,
    pub(crate) initial: Option<Command>,
    pub(crate) options: ProjectOpenOptions,
}
impl ExplorerView {
    #[cfg(feature = "visual-tests")]
    pub(crate) fn capture_ready(&self) -> bool {
        !self.controller.busy()
    }
    pub(crate) fn new(
        controller: ApplicationController,
        executor: Arc<dyn EffectExecutor>,
        launch: ViewLaunch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let ViewLaunch {
            session_path,
            themes: custom_themes,
            initial,
            options: project_options,
        } = launch;
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut themes = vec![Theme::dark(), Theme::light()];
        for theme in custom_themes {
            if !themes.contains(&theme) {
                themes.push(theme);
            }
        }
        let mut view = Self {
            controller,
            executor,
            focus,
            project: ProjectPresenter {
                session_path,
                themes,
                files: vec![],
                symbols: vec![],
                launch_options: project_options.clone(),
            },
            search: SearchState::default(),
            canvas: CanvasState::default(),
            hover: HoverState::default(),
            requests: RequestPresenter {
                status: "Open a project to start exploring.".into(),
                ..Default::default()
            },
            layout: layout::LayoutState::default(),
            sidebar: Default::default(),
            scene: Default::default(),
            close_ready: false,
        };
        cx.observe_window_activation(window, |view, window, cx| {
            if !window.is_window_active() {
                view.cancel_gesture(cx);
                view.clear_hover(cx);
            }
        })
        .detach();
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |view, cx| view.close(window, cx))
                .unwrap_or(true)
        });
        if let Some(command) = initial {
            view.command(command, cx);
        } else if !view
            .controller
            .snapshot()
            .project_root
            .as_os_str()
            .is_empty()
        {
            view.command(Command::Files, cx);
        }
        view
    }
}
