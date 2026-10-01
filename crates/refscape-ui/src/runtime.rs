//! Native platform and window lifecycle; callers never need GPUI types.
use std::{cell::RefCell, path::PathBuf, rc::Rc};

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use refscape_application::{Explorer, LanguageService, SessionRepository};
#[cfg(feature = "visual-tests")]
use refscape_model::Position;
use refscape_model::{ProjectOptions, Theme};

use crate::ExplorerView;

/// Open the explorer window with adapters composed by the executable.
pub fn run<L: LanguageService + 'static, R: SessionRepository + 'static>(
    explorer: Explorer<L, R>,
    session_path: PathBuf,
    themes: Vec<Theme>,
    project: Option<PathBuf>,
) -> Result<(), String> {
    run_with_options(
        explorer,
        session_path,
        themes,
        project,
        ProjectOptions::default(),
    )
}

/// Open the explorer with language and compilation database overrides.
pub fn run_with_options<L: LanguageService + 'static, R: SessionRepository + 'static>(
    explorer: Explorer<L, R>,
    session_path: PathBuf,
    themes: Vec<Theme>,
    project: Option<PathBuf>,
    project_options: ProjectOptions,
) -> Result<(), String> {
    let launch_error = Rc::new(RefCell::new(None));
    let window_error = launch_error.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some("dev.refscape.Refscape".into()),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("Refscape");
                cx.new(|cx| {
                    ExplorerView::new_with_options(
                        explorer,
                        session_path,
                        themes,
                        project,
                        project_options,
                        window,
                        cx,
                    )
                })
            },
        ) {
            *window_error.borrow_mut() = Some(format!("cannot open window: {error}"));
            cx.quit();
        }
        cx.activate(true);
    });
    match launch_error.borrow_mut().take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Render the explorer through the native GPU backend for visual verification.
#[cfg(feature = "visual-tests")]
pub fn render_snapshot<L: LanguageService + 'static, R: SessionRepository + 'static>(
    explorer: Explorer<L, R>,
    session_path: PathBuf,
    output: PathBuf,
) -> Result<(), String> {
    render_snapshot_with_selection(explorer, session_path, output, None)
}

/// Render a selected variable as well as its expanded type cards.
#[cfg(feature = "visual-tests")]
pub fn render_snapshot_with_selection<
    L: LanguageService + 'static,
    R: SessionRepository + 'static,
>(
    mut explorer: Explorer<L, R>,
    session_path: PathBuf,
    output: PathBuf,
    selection: Option<(String, Position)>,
) -> Result<(), String> {
    let inspection = match &selection {
        Some((id, position)) => explorer.inspect_variable(id, *position)?,
        None => None,
    };
    let result = Rc::new(RefCell::new(Ok(())));
    let window_result = result.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let handle = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                show: false,
                focus: false,
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let mut view =
                        ExplorerView::new(explorer, session_path, vec![], None, window, cx);
                    view.inspection = inspection;
                    view.selected = selection.map(|(id, _)| id);
                    view
                })
            },
        ) {
            Ok(handle) => handle,
            Err(error) => {
                *window_result.borrow_mut() = Err(format!("cannot open render window: {error}"));
                cx.quit();
                return;
            }
        };
        if let Err(error) = cx.update_window(handle.into(), |_, window, _| {
            window.resize(size(px(1440.), px(900.)))
        }) {
            *window_result.borrow_mut() = Err(error.to_string());
            cx.quit();
            return;
        }
        cx.spawn(async move |cx| {
            // Let the native resize event initialize the DirectX render target.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(800))
                .await;
            let rendered = cx.update_window(handle.into(), |_, window, cx| {
                let arena = window.draw(cx);
                let saved = (|| {
                    let image = window
                        .render_to_image()
                        .map_err(|error| error.to_string())?;
                    if image.width() < 1000 || image.height() < 600 {
                        return Err("native window has not resized".into());
                    }
                    image.save(&output).map_err(|error| error.to_string())?;
                    println!("Saved {}", output.display());
                    Ok(())
                })();
                arena.clear(cx);
                saved
            });
            *window_result.borrow_mut() =
                rendered.map_err(|error| error.to_string()).and_then(|r| r);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    result.borrow().clone()
}
