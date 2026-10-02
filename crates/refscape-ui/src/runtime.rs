//! Native platform and window lifecycle; callers never need GPUI types.
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use refscape_application::{ApplicationController, Command, EffectExecutor};
#[cfg(feature = "visual-tests")]
use refscape_model::Position;
use refscape_model::{ProjectOpenOptions, Theme};

use crate::view::{ExplorerView, ViewLaunch};

/// Open the explorer with language and compilation database overrides.
pub fn run(
    controller: ApplicationController,
    executor: Arc<dyn EffectExecutor>,
    session_path: PathBuf,
    themes: Vec<Theme>,
    initial: Option<Command>,
    project_options: ProjectOpenOptions,
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
                    ExplorerView::new(
                        controller,
                        executor,
                        ViewLaunch {
                            session_path,
                            themes,
                            initial,
                            options: project_options,
                        },
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

/// Render a selected variable as well as its expanded type cards.
#[cfg(feature = "visual-tests")]
pub fn render_snapshot(
    mut controller: ApplicationController,
    executor: Arc<dyn EffectExecutor>,
    session_path: PathBuf,
    output: PathBuf,
    selection: Option<(String, Position)>,
) -> Result<(), String> {
    let mut inspection = None;
    if let Some((id, position)) = &selection {
        let mut pending = std::collections::VecDeque::from([controller.dispatch(
            refscape_application::Command::Inspect {
                card: id.clone(),
                position: *position,
            },
        )]);
        while let Some(transition) = pending.pop_front() {
            for event in transition.events {
                if let refscape_application::ViewEvent::Inspection(value) = event {
                    inspection = Some(value);
                }
            }
            for effect in transition.effects {
                pending.push_back(controller.complete(executor.execute(effect)));
            }
        }
    }
    let result = Rc::new(RefCell::new(Err(
        "Native frame readiness timed out".to_string()
    )));
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
                    let mut view = ExplorerView::new(
                        controller,
                        executor,
                        ViewLaunch { session_path, themes:vec![], initial:None, options:ProjectOpenOptions::default() },
                        window,
                        cx,
                    );
                    view.canvas.inspection = inspection;
                    view.canvas.selected = selection.map(|(id, _)| id);
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
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut readiness_attempt=0_u32;
            loop {
                readiness_attempt+=1;
                let attempt = cx.update_window(handle.into(), |root, window, cx| {
                    if root.clone().downcast::<ExplorerView>().ok().is_some_and(|view| !view.read(cx).capture_ready()) {return CaptureAttempt::Unready("application effect pending".into());}
                    let arena = window.draw(cx);
                    let captured = window.render_to_image();
                    arena.clear(cx);
                    match captured {
                        Ok(image) if image.width() >= 1000 && image.height() >= 600 => {
                            println!("Native frame: {}x{}, scale_factor={}, readiness_attempt={}; fonts=Cascadia Code,Segoe UI,Consolas",image.width(),image.height(),window.scale_factor(),readiness_attempt);
                            CaptureAttempt::Finished(image.save(&output).map_err(|error|error.to_string()))
                        }
                        Ok(image) => CaptureAttempt::Unready(format!("native target is {}×{}",image.width(),image.height())),
                        Err(error) => {
                            let message=error.to_string();
                            if matches!(message.as_str(),"devices missing"|"resources missing"|"render target missing"|"render_to_image unavailable while recovering from a lost device") {
                                CaptureAttempt::Unready(message)
                            } else { CaptureAttempt::Finished(Err(message)) }
                        }
                    }
                });
                match attempt {
                    Ok(CaptureAttempt::Finished(result)) => { *window_result.borrow_mut()=result; break; }
                    Err(error) => { *window_result.borrow_mut()=Err(error.to_string()); break; }
                    Ok(CaptureAttempt::Unready(message)) if std::time::Instant::now()>=deadline => {
                        *window_result.borrow_mut()=Err(format!("Native frame readiness timed out: {message}")); break;
                    }
                    Ok(CaptureAttempt::Unready(_)) => {
                        // Retry readiness after native events can run. This is a
                        // bounded target check, not a fixed capture delay.
                        cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                    }
                }
            }
            cx.update(|cx|cx.quit());
        }).detach();
    });
    result.borrow().clone()
}

#[cfg(feature = "visual-tests")]
enum CaptureAttempt {
    Finished(Result<(), String>),
    Unready(String),
}
