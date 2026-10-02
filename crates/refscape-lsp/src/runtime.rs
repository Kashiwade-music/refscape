//! One shared process runtime, retained by factories and process supervisors.
use refscape_model::{ErrorKind, OperationContext, RefscapeError};
use std::{
    future::Future,
    process::{Command, Stdio},
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::io::{AsyncRead, AsyncReadExt};
struct RuntimeInner(Option<tokio::runtime::Runtime>);
impl Drop for RuntimeInner {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}
#[derive(Clone)]
pub struct RuntimeOwner(Arc<RuntimeInner>);
impl RuntimeOwner {
    pub fn acquire() -> Self {
        static SHARED: OnceLock<Mutex<Weak<RuntimeInner>>> = OnceLock::new();
        let mut shared = SHARED
            .get_or_init(|| Mutex::new(Weak::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(runtime) = shared.upgrade() {
            return Self(runtime);
        }
        let runtime = Arc::new(RuntimeInner(Some(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("process runtime"),
        )));
        *shared = Arc::downgrade(&runtime);
        Self(runtime)
    }
    pub(crate) fn runtime(&self) -> &tokio::runtime::Runtime {
        self.0.0.as_ref().expect("live process runtime")
    }
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.runtime().block_on(future)
    }
}
pub struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub success: bool,
}
async fn drain(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    let mut exceeded = false;
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let keep = count.min(limit.saturating_sub(output.len()));
        output.extend_from_slice(&buffer[..keep]);
        exceeded |= keep != count;
    }
    Ok((output, exceeded))
}
/// Blocking bridge for workers; pipe draining and child ownership stay on the shared async runtime.
pub fn run_process(
    command: &mut Command,
    context: &OperationContext,
) -> Result<ProcessOutput, RefscapeError> {
    context.check()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let program = command.get_program().to_owned();
    let command = std::mem::replace(command, Command::new(program));
    let runtime = RuntimeOwner::acquire();
    runtime.block_on(async {
        let mut command=tokio::process::Command::from(command);command.kill_on_drop(true);
        let mut child=command.spawn().map_err(|e|RefscapeError::new(ErrorKind::BackendUnavailable,format!("cannot run metadata process: {e}")))?;
        let stdout=child.stdout.take().ok_or_else(||RefscapeError::new(ErrorKind::InternalInvariant,"missing process stdout"))?;
        let stderr=child.stderr.take().ok_or_else(||RefscapeError::new(ErrorKind::InternalInvariant,"missing process stderr"))?;
        let mut out=tokio::spawn(drain(stdout,64*1024*1024));let mut err=tokio::spawn(drain(stderr,1024*1024));
        let status=tokio::select!{biased;
            _=context.cancel.cancelled()=>Err(RefscapeError::new(ErrorKind::Cancelled,"Operation cancelled")),
            _=tokio::time::sleep_until(context.deadline.into())=>Err(RefscapeError::new(ErrorKind::Timeout,"Operation timed out")),
            result=child.wait()=>result.map_err(RefscapeError::from),
        };
        if let Err(error)=status {let _=child.kill().await;let _=child.wait().await;out.abort();err.abort();let _=out.await;let _=err.await;return Err(error);}
        let output=tokio::select!{biased;
            _=context.cancel.cancelled()=>None,
            _=tokio::time::sleep_until(context.deadline.into())=>None,
            output=async{tokio::join!(&mut out,&mut err)}=>Some(output),
        };
        let ((stdout,exceeded),(stderr,_))=match output{
            Some((out,err))=>(out.map_err(|e|RefscapeError::new(ErrorKind::Io,e.to_string()))??,err.map_err(|e|RefscapeError::new(ErrorKind::Io,e.to_string()))??),
            None=>{out.abort();err.abort();let _=out.await;let _=err.await;return context.check().and_then(|()|Err(RefscapeError::new(ErrorKind::Timeout,"process drain deadline exceeded")));},
        };
        if exceeded{return Err(RefscapeError::new(ErrorKind::InvalidData,"cargo metadata exceeds 64 MiB"));}
        Ok(ProcessOutput{stdout,stderr,success:status?.success()})
    })
}
